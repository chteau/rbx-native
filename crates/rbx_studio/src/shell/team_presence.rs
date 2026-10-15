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
use rbx_presence::relay::{RelayConfig, RelayKind};
use rbx_presence::schema::{Snapshot, UserState, HEARTBEAT_MS};

use super::Shell;
use crate::command_bar::Feedback;
use crate::settings::TeamPresenceSettings;
use model::{Collaborator, Departed, EditLog, Member};

/// The environment variable for the canned room (see the module doc).
pub(super) const MOCK_VARIABLE: &str = "RBX_STUDIO_PRESENCE_MOCK";
/// `RBX_STUDIO_PRESENCE_DIFF=1`: alongside the mock room, seed a last-seen
/// snapshot and open the catch-up "View changes" window on a canned diff,
/// so the presence-mode diff can be captured without a live session (the
/// same reason every other `RBX_STUDIO_*` capture variable exists).
const DIFF_VARIABLE: &str = "RBX_STUDIO_PRESENCE_DIFF";
/// How often the worker's events are drained and our own state re-sent if
/// it changed.
const TICK: std::time::Duration = std::time::Duration::from_millis(250);
/// How often, while connected, the last-seen timestamp is persisted, so a
/// crash does not reset catch-up to the whole retention window next time.
const SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Where the connection stands, as the dock's status line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Connection {
    /// Turned off in the dock.
    Off,
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
    /// The relay URL field in the dock's hidden Advanced section.
    pub(super) relay_input: Entity<InputState>,
    /// Whether the Advanced section (the owner's relay setup) is revealed.
    /// Off by default: end users never see or type a URL.
    pub(super) show_advanced: bool,
    /// The transport the owner-setup field will write to the place.
    pub(super) owner_kind: RelayKind,
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
    /// "While you were away": the value-less edit summary fetched once on
    /// connect, until dismissed. `None` when there is nothing to catch up on.
    pub(super) catchup: Option<rbx_presence::catchup::Summary>,
    /// Set when the user dismisses the catch-up block, so a later refresh
    /// does not bring it back.
    pub(super) catchup_dismissed: bool,
    /// Our Roblox display name, once looked up; sent instead of the id.
    own_name: Option<String>,
    /// What was last handed to the worker, and when.
    last_sent: Option<(UserState, Instant)>,
    /// The place we are connected to, for persisting last-seen state.
    place_id: Option<u64>,
    /// The place as this client last saw it, loaded from the last-seen store
    /// on connect — the "before" that a catch-up diff ("View changes")
    /// compares the freshly loaded place against. `None` when nothing was
    /// stored or it could not be decoded.
    pub(super) before: Option<rbx_dom::WeakDom>,
    /// When the last-seen timestamp was last persisted, to throttle it.
    last_saved: Option<Instant>,
    /// The open "View changes" window (the catch-up diff), if any.
    pub(super) presence_diff: Option<WindowHandle<gpui_kit::component::Root>>,
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
                .placeholder("Relay URL (blank = community default)")
                .default_value(settings.relay_url.clone())
        });
        TeamPresence {
            settings,
            connection: Connection::Off,
            relay_input,
            show_advanced: false,
            owner_kind: RelayKind::Worker,
            client: None,
            watch: None,
            snapshot: None,
            members: None,
            departed: Vec::new(),
            collaborators: Vec::new(),
            edits: EditLog::default(),
            camera: None,
            catchup: None,
            catchup_dismissed: false,
            own_name: None,
            last_sent: None,
            place_id: None,
            before: None,
            last_saved: None,
            presence_diff: None,
            scroll: ScrollHandle::new(),
        }
    }
}

/// Two small DOMs whose diff is a representative catch-up: a changed
/// property, a changed script source, and an added part. Used only by the
/// `DIFF_VARIABLE` capture path.
fn synthetic_diff_doms() -> (rbx_dom::WeakDom, rbx_dom::WeakDom) {
    use rbx_dom::{Variant, WeakDom};
    let build = |transparency: f32, source: &str, extra: bool| {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Platform", Some(workspace));
        dom.get_mut(part)
            .unwrap()
            .properties_mut()
            .insert("Transparency".into(), Variant::Float32(transparency));
        let script = dom.new_instance("Script", "Main", Some(workspace));
        dom.get_mut(script)
            .unwrap()
            .properties_mut()
            .insert("Source".into(), Variant::String(source.into()));
        if extra {
            dom.new_instance("Part", "Ramp", Some(workspace));
        }
        dom
    };
    let before = build(0.0, "print(\"hello\")\n", false);
    let after = build(0.5, "print(\"hello\")\nprint(\"world\")\n", true);
    (before, after)
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
                    // The canned room's edits double as a catch-up summary, so
                    // the "While you were away" block screenshots from the mock.
                    let entries: Vec<rbx_presence::catchup::JournalEntry> = mock
                        .snapshot
                        .users
                        .iter()
                        .flat_map(|u| {
                            u.edits
                                .iter()
                                .map(move |e| rbx_presence::catchup::JournalEntry {
                                    user_id: u.user_id,
                                    name: u.name.clone(),
                                    path: e.path.clone(),
                                    property: e.property.clone(),
                                    at: e.at,
                                })
                        })
                        .collect();
                    let summary = rbx_presence::catchup::summarize(&entries);
                    let presence = &mut self.team_presence;
                    if !summary.authors.is_empty() {
                        presence.catchup = Some(summary);
                    }
                    presence.snapshot = Some(mock.snapshot);
                    presence.members = Some(mock.members);
                    presence.departed = mock.departed;
                    presence.connection = Connection::Mock;
                    self.set_panel_open(super::layout::Panel::Collaborators, true, cx);
                    self.refresh_collaborators(cx);
                    self.keep_mock_alive(cx);
                    self.apply_debug_presence_diff(cx);
                    cx.notify();
                    return;
                }
                Err(why) => self.output.push(
                    "Presence",
                    Feedback::Error(format!("{MOCK_VARIABLE}: {why}")),
                ),
            }
        }
        if let Some(config) = rbx_presence::secret::read_relay(&self.dom) {
            self.team_presence.owner_kind = config.kind;
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
                // Persist the snapshot before tearing down, so the next
                // connect can diff against where the place was left.
                self.save_last_seen(true);
                self.disconnect_presence(cx);
                self.team_presence.connection = Connection::Off;
            }
        }
        cx.notify();
    }

    /// Owner setup: writes the chosen relay (kind + URL) into the place, the
    /// way the secret is written, so every collaborator reads it and connects
    /// with nothing to type. An empty URL clears it (back to the community
    /// default). Reconnects when presence is on.
    pub(super) fn save_presence_relay(&mut self, cx: &mut Context<Self>) {
        let url = self
            .team_presence
            .relay_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        // Keep the typed URL as a convenience so the field survives a reload.
        self.team_presence.settings.relay_url = url.clone();
        self.save_settings();

        self.push_history();
        if url.is_empty() {
            rbx_presence::secret::clear_relay(&mut self.dom);
        } else {
            let config = RelayConfig {
                kind: self.team_presence.owner_kind,
                url,
            };
            if rbx_presence::secret::write_relay(&mut self.dom, &config).is_none() {
                self.output.push(
                    "Presence",
                    Feedback::Error(
                        "This place has no ServerStorage to keep the relay setting in.".into(),
                    ),
                );
                return;
            }
        }
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.output.push(
            "Presence",
            Feedback::Info(
                "Saved the relay to this place. Save or Publish once so collaborators get it."
                    .into(),
            ),
        );
        if self.team_presence.settings.enabled && self.team_presence.connection != Connection::Mock
        {
            self.connect_presence(cx);
        }
        cx.notify();
    }

    /// Owner setup: cycles the transport the Save button will write.
    pub(super) fn toggle_owner_kind(&mut self, cx: &mut Context<Self>) {
        self.team_presence.owner_kind = match self.team_presence.owner_kind {
            RelayKind::Worker => RelayKind::Ntfy,
            RelayKind::Ntfy => RelayKind::Worker,
        };
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
        presence.catchup = None;
        presence.place_id = None;
        presence.before = None;
        presence.last_saved = None;
        self.viewport
            .update(cx, |view, _| view.set_presence(Vec::new()));
    }

    fn connect_presence(&mut self, cx: &mut Context<Self>) {
        self.disconnect_presence(cx);
        let place = crate::home::link_of(&self.path).map(|link| link.place_id);
        let user = self.studio_presence.user_id;
        let blocked = place
            .is_none()
            .then_some(Connection::NoPlace)
            .or_else(|| user.is_none().then_some(Connection::NoUser));
        if let Some(state) = blocked {
            self.team_presence.connection = state;
            cx.notify();
            return;
        }
        let (Some(place), Some(user_id)) = (place, user) else {
            return;
        };
        // Zero-config: the first connector mints the place's secret; everyone
        // else reads it. Writing it needs a Save/Publish to reach Studio, so
        // `write_new_secret` says so.
        let secret = match rbx_presence::secret::read(&self.dom) {
            Some(secret) => secret,
            None => match self.write_new_secret(cx) {
                Some(secret) => secret,
                None => {
                    self.team_presence.connection = Connection::NoSecret;
                    cx.notify();
                    return;
                }
            },
        };
        // The place names the relay; collaborators read it and connect with
        // nothing to type. A place that names none falls back to the
        // best-effort community instance.
        let config = rbx_presence::secret::read_relay(&self.dom)
            .unwrap_or_else(rbx_presence::relay::RelayConfig::community);
        let key = rbx_presence::auth::derive_key(&secret, place);
        let room = rbx_presence::auth::room_id(&key);
        let client_config = rbx_presence::client::Config {
            kind: config.kind,
            relay_url: config.trimmed_url().to_owned(),
            room,
            key,
            user_id,
        };
        // Catch up only on what happened since this client last left this
        // place, and keep the snapshot it left behind for "View changes".
        let record =
            Self::presence_cache_dir().and_then(|dir| rbx_presence::lastseen::load(&dir, place));
        let since = record.as_ref().map_or(0, |r| r.journal_ms);
        self.team_presence.before = record
            .and_then(|r| r.snapshot)
            .and_then(|bytes| rbx_binary::deserialize(&bytes).ok());
        self.team_presence.place_id = Some(place);
        self.team_presence.last_saved = None;
        self.fetch_catchup(client_config.clone(), since, cx);
        let (client, events) = Client::start(client_config);
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

    /// Fetches the "while you were away" summary once, off the UI thread, and
    /// shows it if it has anything and the user has not dismissed one.
    /// `since_ms` is the timestamp this client last caught up to for this
    /// place (0 the first time, so everything the transport still retains).
    fn fetch_catchup(
        &mut self,
        config: rbx_presence::client::Config,
        since_ms: u64,
        cx: &mut Context<Self>,
    ) {
        self.team_presence.catchup = None;
        self.team_presence.catchup_dismissed = false;
        cx.spawn(async move |this, cx| {
            let summary = cx
                .background_spawn(async move { rbx_presence::catchup::fetch(&config, since_ms) })
                .await;
            if let Ok(summary) = summary {
                if !summary.authors.is_empty() {
                    let _ = this.update(cx, |shell, cx| {
                        if !shell.team_presence.catchup_dismissed {
                            shell.team_presence.catchup = Some(summary);
                            cx.notify();
                        }
                    });
                }
            }
        })
        .detach();
    }

    /// Capture aid: with `DIFF_VARIABLE` set, seeds a last-seen snapshot
    /// and opens the "View changes" window on a canned, self-contained diff.
    fn apply_debug_presence_diff(&mut self, cx: &mut Context<Self>) {
        if std::env::var(DIFF_VARIABLE).as_deref() != Ok("1") {
            return;
        }
        let (before, after) = synthetic_diff_doms();
        let nodes = rbx_dom::diff::diff(&before, &after);
        // Keep the snapshot so the dock shows the "View changes" link too.
        self.team_presence.before = Some(before);
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = crate::shell::argon_diff_window::ArgonDiffWindow::open_presence(
                shell.clone(),
                nodes,
                cx,
            );
            shell.update(cx, |shell, _| shell.team_presence.presence_diff = opened);
        });
    }

    /// Opens the catch-up diff ("View changes"): the place as this client
    /// last saw it against the place as it is now. Raises the window if one
    /// is already open; says so when nothing changed.
    pub(super) fn open_presence_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.team_presence.presence_diff {
            let _ = existing.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let Some(before) = self.team_presence.before.clone() else {
            return;
        };
        let nodes = rbx_dom::diff::diff(&before, &self.dom);
        if nodes.is_empty() {
            self.output.push(
                "Presence",
                Feedback::Info("No changes to the place since you were last here.".into()),
            );
            return;
        }
        let shell = cx.entity();
        // Deferred like `open_argon_diff`: opening a window renders it at
        // once, and this runs inside the click handler's own `Shell` update.
        cx.defer(move |cx| {
            let opened = crate::shell::argon_diff_window::ArgonDiffWindow::open_presence(
                shell.clone(),
                nodes,
                cx,
            );
            shell.update(cx, |shell, _| shell.team_presence.presence_diff = opened);
        });
    }

    /// Dismisses the catch-up block; it does not come back this session.
    pub(super) fn dismiss_catchup(&mut self, cx: &mut Context<Self>) {
        self.team_presence.catchup = None;
        self.team_presence.catchup_dismissed = true;
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

        // Keep the last-seen timestamp fresh while connected, so catch-up
        // after a crash still starts from roughly where we are now.
        let due_save = self
            .team_presence
            .last_saved
            .is_none_or(|at| at.elapsed() >= SAVE_EVERY);
        if self.team_presence.connection == Connection::Connected && due_save {
            self.save_last_seen(false);
        }
    }

    /// Where per-place last-seen state is kept (under the editor's config
    /// dir). `None` when no config directory can be determined.
    fn presence_cache_dir() -> Option<std::path::PathBuf> {
        crate::settings::default_config_dir().map(|dir| dir.join("presence"))
    }

    /// Persists the connected place's last-seen timestamp, and its snapshot
    /// when `snapshot` is set. Best effort: a store that cannot be written
    /// (no config dir, encode failure) is skipped — it only costs a wider
    /// catch-up next time.
    fn save_last_seen(&mut self, snapshot: bool) {
        let Some(place) = self.team_presence.place_id else {
            return;
        };
        let Some(dir) = Self::presence_cache_dir() else {
            return;
        };
        let bytes = snapshot
            .then(|| rbx_binary::serialize(&self.dom).ok())
            .flatten();
        if rbx_presence::lastseen::save(&dir, place, now_ms(), bytes.as_deref()).is_ok() {
            self.team_presence.last_saved = Some(Instant::now());
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
    /// and returns it, or `None` when the place has no `ServerStorage` to
    /// keep it in. Studio collaborators get it once the place is saved or
    /// published, so this says so once.
    fn write_new_secret(&mut self, cx: &mut Context<Self>) -> Option<String> {
        self.push_history();
        let secret = rbx_presence::secret::generate();
        if rbx_presence::secret::write(&mut self.dom, &secret).is_none() {
            self.output.push(
                "Presence",
                Feedback::Error(
                    "This place has no ServerStorage to keep the presence key in.".into(),
                ),
            );
            return None;
        }
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.output.push(
            "Presence",
            Feedback::Info(
                "Shared a presence key for this place. Save or Publish once so Studio collaborators get it."
                    .into(),
            ),
        );
        Some(secret)
    }

    /// The dock's Advanced section reveal.
    pub(super) fn toggle_presence_advanced(&mut self, cx: &mut Context<Self>) {
        self.team_presence.show_advanced = !self.team_presence.show_advanced;
        cx.notify();
    }
}
