//! Team presence through the rbx-native presence plugin: collaborators who
//! run the plugin in real Roblox Studio send where they are (selection,
//! camera, open script, recent edits) to a relay, and so does this editor.
//! Official APIs only — nothing here speaks Roblox's Team Create protocol —
//! which is also the catch: someone shows up only while they have Studio
//! open with the plugin running, and the dock says so in plain words.
//!
//! The room is the place's: its secret lives in the place itself (see
//! `rbx_presence::secret`), so only people who can open the place can
//! join. A place without one can get one here, which needs a save or
//! publish to reach Studio.
//!
//! `RBX_STUDIO_PRESENCE_MOCK=fixture` (or a path to a file of the same
//! shape) shows a canned room without any network, for screenshots.

mod hooks;
mod model;
mod panel;
mod viewport;

pub(super) use hooks::PresenceMark;

use std::time::Instant;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_presence::client::{Client, Event};
use rbx_presence::schema::{Snapshot, UserState, HEARTBEAT_MS};

use super::Shell;
use crate::command_bar::Feedback;
use crate::settings::TeamPresenceSettings;
use model::{Collaborator, Departed, EditLog, Member};

/// The environment variable for the canned room (see the module doc).
pub(super) const MOCK_VARIABLE: &str = "RBX_STUDIO_PRESENCE_MOCK";
/// How often the worker's events are drained and our own state re-sent if
/// it changed.
const TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// Where the connection stands, as the dock's status line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Connection {
    /// Turned off in the dock.
    Off,
    /// On, but no relay URL is set.
    NoRelay,
    /// On, but the open file is not linked to a Roblox place.
    NoPlace,
    /// On, but the place has no presence secret yet.
    NoSecret,
    /// On, but not signed in to Roblox, so there is no user id to send.
    NoUser,
    Connecting,
    Connected,
    /// The relay refused the key or the signature.
    Rejected(String),
    /// Lost; the worker is retrying.
    Retrying(String),
    /// `RBX_STUDIO_PRESENCE_MOCK`: a canned room, no network.
    Mock,
}

pub(super) struct TeamPresence {
    /// Persisted; `enabled` is the opt-in.
    pub(super) settings: TeamPresenceSettings,
    pub(super) connection: Connection,
    /// The relay URL field in the dock.
    pub(super) relay_input: Entity<InputState>,
    client: Option<Client>,
    /// Drains `client`'s events every `TICK`; dropped with it.
    watch: Option<Task<()>>,
    snapshot: Option<Snapshot>,
    /// Roblox's Team Create member list, when something supplies one (the
    /// mock does); without it nobody is "in session without the plugin".
    members: Option<Vec<Member>>,
    departed: Vec<Departed>,
    /// Recomputed whenever the snapshot or the place's tree changes.
    pub(super) collaborators: Vec<Collaborator>,
    edits: EditLog,
    /// Our own camera, from the last `PoseSynced`.
    camera: Option<[f64; 12]>,
    /// Our Roblox display name, once looked up; sent instead of the id.
    own_name: Option<String>,
    /// What was last handed to the worker, and when.
    last_sent: Option<(UserState, Instant)>,
    pub(super) scroll: ScrollHandle,
}

impl TeamPresence {
    pub(super) fn new(
        settings: TeamPresenceSettings,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        let relay_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://relay.example.workers.dev")
                .default_value(settings.relay_url.clone())
        });
        TeamPresence {
            settings,
            connection: Connection::Off,
            relay_input,
            client: None,
            watch: None,
            snapshot: None,
            members: None,
            departed: Vec::new(),
            collaborators: Vec::new(),
            edits: EditLog::default(),
            camera: None,
            own_name: None,
            last_sent: None,
            scroll: ScrollHandle::new(),
        }
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Shell {
    /// Startup: the mock room when `MOCK_VARIABLE` is set (opening the
    /// Collaborators dock), else connects when the setting is on.
    pub(super) fn init_team_presence(&mut self, cx: &mut Context<Self>) {
        if let Ok(spec) = std::env::var(MOCK_VARIABLE) {
            match model::load_mock(&spec, now_ms()) {
                Ok(mock) => {
                    let presence = &mut self.team_presence;
                    presence.snapshot = Some(mock.snapshot);
                    presence.members = Some(mock.members);
                    presence.departed = mock.departed;
                    presence.connection = Connection::Mock;
                    self.set_panel_open(super::layout::Panel::Collaborators, true, cx);
                    self.refresh_collaborators(cx);
                    self.keep_mock_alive(cx);
                    cx.notify();
                    return;
                }
                Err(why) => self.output.push(
                    "Presence",
                    Feedback::Error(format!("{MOCK_VARIABLE}: {why}")),
                ),
            }
        }
        if self.team_presence.settings.enabled {
            self.connect_presence(cx);
        }
    }

    /// The dock's on/off switch; saves settings, connects or disconnects.
    pub(super) fn set_team_presence_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.team_presence.settings.enabled = enabled;
        self.save_settings();
        if self.team_presence.connection != Connection::Mock {
            if enabled {
                self.connect_presence(cx);
            } else {
                self.disconnect_presence(cx);
                self.team_presence.connection = Connection::Off;
            }
        }
        cx.notify();
    }

    /// The dock's relay URL field was applied; saves and reconnects.
    pub(super) fn apply_presence_relay_url(&mut self, cx: &mut Context<Self>) {
        let url = self
            .team_presence
            .relay_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        self.team_presence.settings.relay_url = url;
        self.save_settings();
        if self.team_presence.settings.enabled && self.team_presence.connection != Connection::Mock
        {
            self.connect_presence(cx);
        }
        cx.notify();
    }

    /// The canned users never send, so their `seen` is moved along here;
    /// otherwise they would expire like anyone who stopped sending.
    fn keep_mock_alive(&mut self, cx: &mut Context<Self>) {
        self.team_presence.watch = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            let alive = this.update(cx, |shell, cx| {
                let now = now_ms();
                if let Some(snapshot) = &mut shell.team_presence.snapshot {
                    snapshot.users.iter_mut().for_each(|user| user.seen = now);
                }
                if shell.refresh_collaborators(cx) {
                    cx.notify();
                }
            });
            if alive.is_err() {
                return;
            }
        }));
    }

    fn disconnect_presence(&mut self, cx: &mut Context<Self>) {
        let presence = &mut self.team_presence;
        presence.client = None;
        presence.watch = None;
        presence.snapshot = None;
        presence.departed.clear();
        presence.collaborators.clear();
        presence.last_sent = None;
        self.viewport
            .update(cx, |view, _| view.set_presence(Vec::new()));
    }

    fn connect_presence(&mut self, cx: &mut Context<Self>) {
        self.disconnect_presence(cx);
        let url = self.team_presence.settings.relay_url.trim().to_string();
        let blocked = if url.is_empty() {
            Some(Connection::NoRelay)
        } else {
            None
        };
        let place = crate::home::link_of(&self.path).map(|link| link.place_id);
        let secret = rbx_presence::secret::read(&self.dom);
        let user = self.studio_presence.user_id;
        let blocked = blocked
            .or_else(|| place.is_none().then_some(Connection::NoPlace))
            .or_else(|| secret.is_none().then_some(Connection::NoSecret))
            .or_else(|| user.is_none().then_some(Connection::NoUser));
        if let Some(state) = blocked {
            self.team_presence.connection = state;
            cx.notify();
            return;
        }
        let (Some(place), Some(secret), Some(user_id)) = (place, secret, user) else {
            return;
        };
        let key = rbx_presence::auth::derive_key(&secret, place);
        let room = rbx_presence::auth::room_id(&key);
        let (client, events) = Client::start(rbx_presence::client::Config {
            relay_url: url,
            room,
            key,
            user_id,
        });
        self.team_presence.client = Some(client);
        self.team_presence.connection = Connection::Connecting;
        if self.team_presence.own_name.is_none() {
            self.look_up_own_name(user_id, cx);
        }
        self.team_presence.watch = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this
                .update(cx, |shell, cx| shell.presence_tick(&events, user_id, cx))
                .is_err()
            {
                return;
            }
        }));
        cx.notify();
    }

    fn look_up_own_name(&mut self, user_id: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let name = cx
                .background_spawn(async move {
                    rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config())
                        .user_display_name(user_id)
                })
                .await;
            if let Ok(name) = name {
                let _ = this.update(cx, |shell, _| shell.team_presence.own_name = Some(name));
            }
        })
        .detach();
    }

    fn presence_tick(
        &mut self,
        events: &std::sync::mpsc::Receiver<Event>,
        user_id: u64,
        cx: &mut Context<Self>,
    ) {
        let now = now_ms();
        let mut changed = false;
        while let Ok(event) = events.try_recv() {
            let presence = &mut self.team_presence;
            match event {
                Event::Connected => presence.connection = Connection::Connected,
                Event::Snapshot(next) => {
                    if let Some(previous) = &presence.snapshot {
                        presence
                            .departed
                            .extend(model::departures(previous, &next, user_id, now));
                    }
                    presence.snapshot = Some(next);
                }
                Event::Disconnected(why) => presence.connection = Connection::Retrying(why),
                Event::Rejected(why) => presence.connection = Connection::Rejected(why),
            }
            changed = true;
        }

        self.team_presence
            .departed
            .retain(|gone| now.saturating_sub(gone.left_at) < model::DEPARTED_KEEP_MS);
        let state = self.own_state(user_id, now);
        let presence = &mut self.team_presence;
        let due = match &presence.last_sent {
            Some((last, at)) => *last != state || at.elapsed().as_millis() as u64 >= HEARTBEAT_MS,
            None => true,
        };
        if due {
            if let Some(client) = &presence.client {
                client.publish(state.clone());
            }
            presence.last_sent = Some((state, Instant::now()));
        }

        changed |= self.refresh_collaborators(cx);
        if changed {
            cx.notify();
        }
    }

    fn own_state(&self, user_id: u64, now: u64) -> UserState {
        let dom = &self.dom;
        let path = |r| rbx_presence::paths::path_of(dom, r);
        let name = match &self.team_presence.own_name {
            Some(name) => name.clone(),
            None => format!("User {user_id}"),
        };
        model::own_state(
            user_id,
            &name,
            self.selected_all()
                .iter()
                .filter_map(|&r| path(r))
                .collect(),
            self.team_presence.camera,
            self.scripts.tabs.active().and_then(path),
            self.team_presence.edits.recent(now),
        )
    }

    /// Recomputes the collaborators; when they differ, updates the viewport
    /// layer too. Returns whether anything changed.
    fn refresh_collaborators(&mut self, cx: &mut Context<Self>) -> bool {
        let presence = &self.team_presence;
        let own = self.studio_presence.user_id.unwrap_or(0);
        let next = model::collaborators(
            presence.snapshot.as_ref(),
            presence.members.as_deref(),
            &presence.departed,
            own,
            &self.dom,
            now_ms(),
        );
        if next == presence.collaborators {
            return false;
        }
        let segments = viewport::segments(&self.dom, &self.database, &next);
        self.team_presence.collaborators = next;
        self.viewport
            .update(cx, |view, _| view.set_presence(segments));
        true
    }

    /// Puts a new secret into the place (an undoable edit, like an insert)
    /// and reconnects. Studio collaborators get it once the place is saved
    /// or published.
    pub(super) fn create_presence_secret(&mut self, cx: &mut Context<Self>) {
        self.push_history();
        let secret = rbx_presence::secret::generate();
        if rbx_presence::secret::write(&mut self.dom, &secret).is_none() {
            self.output.push(
                "Presence",
                Feedback::Error(
                    "This place has no ServerStorage to keep the presence key in.".into(),
                ),
            );
            return;
        }
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.output.push(
            "Presence",
            Feedback::Info(
                "Presence key created. Studio collaborators get it after you Save or Publish the place."
                    .into(),
            ),
        );
        if self.team_presence.settings.enabled && self.team_presence.connection != Connection::Mock
        {
            self.connect_presence(cx);
        }
        cx.notify();
    }
}
