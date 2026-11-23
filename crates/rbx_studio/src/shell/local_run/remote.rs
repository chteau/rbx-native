//! Collaborators in the co-edit room joining a Server & Clients session as
//! players (Beta Features › Local Run and Live co-editing, both on), over
//! the room's own relay (`rbx_presence::play`, sealed with the place key),
//! never Roblox's Team Create.
//!
//! - Host: while a Server & Clients session runs, this editor tells the
//!   room every couple of seconds (`Down::Open`). A collaborator's ask
//!   shows in the test window's tab row as an offer; accepted, they join
//!   the session wearing the avatar they sent and get a Player tab here
//!   like a test player's, which the host can watch, but whose keys and
//!   mouse are the collaborator's. Each frame their client's changes go to
//!   them with the values they ended on (`rbx_sim::remote`). End Session
//!   is the host's alone; it tells every collaborator.
//! - Collaborator: the Collaborators dock offers "Join as player" for each
//!   session announced. Their avatar is the one Play (local) would use
//!   (signed in: theirs; else the noob). Once let in, a window shows their
//!   player's view, built and kept up from the frames, and their input
//!   goes up as the same actions a recording keeps. Exit, End Session or
//!   closing the window leaves.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_dom::{Change, Ref};
use rbx_presence::client::Route;
use rbx_presence::play::{self, Event, Link};
use rbx_presence::relay::RelayKind;
use rbx_sim::remote::{self as wire, Down, Frame, Up, Viewer};
use rbx_sim::replay::Action;
use rbx_sim::Peer;

use super::avatars;
use super::tabs::TestWindow;
use super::windows::{Home, PeerLink, Shown};
use super::{push_row, MAX_PLAYERS};
use crate::command_bar::Feedback;
use crate::shell::Shell;
use crate::tokens;
use crate::workspace_view::core_gui::CoreAction;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;

/// How often the connection is read and the collaborator's input sent.
const PUMP: Duration = Duration::from_millis(16);
/// How often a host tells the room its session takes players.
const OPEN_EVERY: Duration = Duration::from_secs(2);
/// A session not heard of for this long is gone.
const HOST_GONE: Duration = Duration::from_secs(6);
/// Tries on this computer's loopback port before going through the
/// tunnel, for a host whose relay didn't get its usual port.
const LOOPBACK_TRIES: u32 = 2;

/// A session someone in the room announced.
pub(in crate::shell) struct Announced {
    pub(in crate::shell) from: u64,
    pub(in crate::shell) name: String,
    seen: Instant,
}

/// This editor playing someone else's session.
pub(in crate::shell) struct Guest {
    host: u64,
    pub(in crate::shell) host_name: String,
    viewer: Viewer,
    /// Their `Player`, once let in.
    player: Option<Ref>,
    window: Option<AnyWindowHandle>,
    shell: WeakEntity<Shell>,
    /// Frames come with the core GUI only when it changed.
    core: Option<rbx_sim::gui::CoreGui>,
    /// Frames that arrived since the last pump.
    frames: Vec<Frame>,
    /// The screen size and insets last sent, and the menu.
    sent: Option<([f32; 2], [f32; 4])>,
    menu_open: bool,
    since: Instant,
    /// Bytes both ways when they were let in.
    base: (u64, u64),
}

impl Guest {
    pub(in crate::shell) fn joined(&self) -> bool {
        self.player.is_some()
    }
}

#[derive(Default)]
pub(crate) struct RemotePlay {
    link: Option<(String, Link)>,
    events: Option<Receiver<Event>>,
    pid: u64,
    connected: bool,
    /// The connection dropped since it was made: what was in flight is
    /// lost, the next one starts the tree over.
    lost: bool,
    /// Tries that never connected, for the loopback fallback.
    failures: u32,
    through_tunnel: bool,
    pump: Option<Task<()>>,
    last_open: Option<Instant>,
    told_transport: bool,
    /// Since the first collaborator was let in, and the bytes then.
    hosting: Option<(Instant, (u64, u64))>,
    pub(in crate::shell) hosts: Vec<Announced>,
    pub(in crate::shell) guest: Option<Guest>,
}

impl RemotePlay {
    pub(in crate::shell) fn send_down(&self, to: u64, message: &Down) {
        if let Some((_, link)) = &self.link {
            link.send(to, wire::encode_down(message));
        }
    }

    fn send_up(&self, to: u64, message: &Up) {
        if let Some((_, link)) = &self.link {
            link.send(to, wire::encode_up(message));
        }
    }

    /// What hosting collaborators cost, for the Output once the session
    /// ends.
    pub(in crate::shell) fn hosting_note(&mut self) -> Option<String> {
        let (since, (sent, received)) = self.hosting.take()?;
        let (now_sent, now_received) = self.traffic();
        let seconds = since.elapsed().as_secs_f64();
        Some(format!(
            "Collaborators played for {seconds:.0} s: {:.1} KB/s out, {:.2} KB/s in",
            rate(now_sent.saturating_sub(sent), seconds),
            rate(now_received.saturating_sub(received), seconds)
        ))
    }

    /// Bytes both ways so far.
    fn traffic(&self) -> (u64, u64) {
        self.link.as_ref().map_or((0, 0), |(_, link)| {
            let stats = link.stats();
            (
                stats.sent.load(std::sync::atomic::Ordering::Relaxed),
                stats.received.load(std::sync::atomic::Ordering::Relaxed),
            )
        })
    }
}

/// `shown` (a collaborator's tab on the host) as the frame their editor
/// gets; the ops are the session's to fill in.
pub(super) fn frame_of(shown: &Shown) -> Frame {
    Frame {
        camera: shown.camera,
        mouse: shown.mouse,
        player_gui: shown.player_gui,
        core: shown.core.clone(),
        chat: shown.chat.clone(),
        notices: shown.notices.clone(),
        dialog: shown.dialog.clone(),
        bursts: shown.bursts.clone(),
        rows: shown
            .rows
            .iter()
            .flat_map(|(_, output)| {
                output
                    .lines()
                    .iter()
                    .zip(output.kinds())
                    .map(|(line, kind)| (*kind, line.clone()))
            })
            .collect(),
        paused: shown.paused,
        ..Frame::default()
    }
}

/// KB/s for `bytes` over `seconds`.
fn rate(bytes: u64, seconds: f64) -> f64 {
    bytes as f64 / seconds.max(0.001) / 1000.0
}

/// Whether collaborators can play a Local Run together: it takes both the
/// Local Run and the co-edit betas. Off, the Collaborators dock offers no
/// "Join as player".
pub(in crate::shell) fn betas_on() -> bool {
    crate::beta::enabled(super::BETA_ID) && crate::beta::enabled(crate::shell::coedit::BETA_ID)
}

impl Shell {
    /// Where this editor's play connection should go now: the hosted
    /// room's relay, while both betas are on and co-editing is connected.
    fn remote_play_target(&mut self) -> Option<play::Config> {
        if !betas_on() {
            return None;
        }
        let seed = self.team_presence.coedit.as_ref()?.seed();
        if seed.kind != RelayKind::Hosted {
            if !self.play.live.local.remote.told_transport {
                self.play.live.local.remote.told_transport = true;
                push_row(
                    &mut self.output,
                    Feedback::Warning(
                        "Collaborators can only join a Local Run through the hosted relay (the default); this place names another.".into(),
                    ),
                );
            }
            return None;
        }
        let remote = &self.play.live.local.remote;
        let relay_url = match self.team_presence.route.as_ref()? {
            // The relay runs here: its loopback port is one hop shorter
            // than the tunnel there and back.
            Route::Hosting(public) if remote.through_tunnel => public.clone(),
            Route::Hosting(_) => format!(
                "http://127.0.0.1:{}",
                rbx_presence::announce::loopback_port(&seed.room)
            ),
            Route::Joined(url) => url.clone(),
            Route::Waiting => return None,
        };
        Some(play::Config {
            relay_url: relay_url.trim_end_matches('/').to_owned(),
            room: seed.room.clone(),
            key: seed.key.clone(),
            enc_key: seed.enc_key,
            user_id: seed.user_id,
            pid: if remote.pid == 0 {
                play::new_pid()
            } else {
                remote.pid
            },
        })
    }

    /// Opens, moves or closes the play connection to follow the room
    /// (every presence tick).
    pub(in crate::shell) fn remote_play_tick(&mut self, cx: &mut Context<Self>) {
        let target = self.remote_play_target();
        let remote = &mut self.play.live.local.remote;
        let current = remote.link.as_ref().map(|(url, _)| url.clone());
        match target {
            None if current.is_some() => {
                self.remote_guest_end("The room\u{2019}s connection closed.", cx);
                let remote = &mut self.play.live.local.remote;
                remote.link = None;
                remote.events = None;
                remote.pump = None;
                remote.hosts.clear();
                cx.notify();
            }
            Some(config) if current.as_deref() != Some(config.relay_url.as_str()) => {
                remote.pid = config.pid;
                remote.connected = false;
                remote.lost = false;
                remote.failures = 0;
                let url = config.relay_url.clone();
                let (link, events) = Link::start(config);
                remote.link = Some((url, link));
                remote.events = Some(events);
                remote.pump = Some(cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(PUMP).await;
                    if this
                        .update(cx, |shell, cx| shell.remote_play_pump(cx))
                        .is_err()
                    {
                        return;
                    }
                }));
            }
            _ => {}
        }
    }

    /// Reads what arrived, tells the room about a session taking players,
    /// and plays the collaborator's side.
    fn remote_play_pump(&mut self, cx: &mut Context<Self>) {
        let mut messages = Vec::new();
        if let Some(events) = &self.play.live.local.remote.events {
            while let Ok(event) = events.try_recv() {
                messages.push(event);
            }
        }
        for event in messages {
            match event {
                Event::Connected => self.remote_play_connected(),
                Event::Disconnected(_) => {
                    let remote = &mut self.play.live.local.remote;
                    remote.lost |= remote.connected;
                    if !remote.connected {
                        remote.failures += 1;
                    }
                    remote.connected = false;
                    // The loopback port wasn't this host's after all.
                    if remote.failures >= LOOPBACK_TRIES && !remote.through_tunnel {
                        remote.through_tunnel = true;
                        self.remote_play_tick(cx);
                    }
                }
                Event::Message { from, bytes } => {
                    if let Ok(up) = wire::decode_up(&bytes) {
                        self.remote_host_receive(from, up, cx);
                    } else if let Ok(down) = wire::decode_down(&bytes) {
                        self.remote_guest_receive(from, down, cx);
                    }
                }
            }
        }
        self.remote_host_announce();
        let now = Instant::now();
        let remote = &mut self.play.live.local.remote;
        let before = remote.hosts.len();
        remote
            .hosts
            .retain(|h| now.duration_since(h.seen) < HOST_GONE);
        if remote.hosts.len() != before {
            cx.notify();
        }
        self.remote_guest_play(cx);
    }

    fn remote_play_connected(&mut self) {
        let remote = &mut self.play.live.local.remote;
        remote.connected = true;
        remote.failures = 0;
        if !std::mem::take(&mut remote.lost) {
            return;
        }
        // Frames went missing: everyone starts the tree over.
        if let Some(guest) = remote.guest.as_ref().filter(|g| g.joined()) {
            remote.send_up(guest.host, &Up::Resync);
        }
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.remote.resync(None);
        }
    }

    // --- The host's side ---

    /// Tells the room, now and then, that this editor's Server & Clients
    /// session takes players.
    fn remote_host_announce(&mut self) {
        let taking = self
            .play
            .live
            .local
            .running
            .as_ref()
            .is_some_and(|run| run.windowed && run.replay.is_none());
        let remote = &self.play.live.local.remote;
        if !taking
            || !remote.connected
            || remote.last_open.is_some_and(|at| at.elapsed() < OPEN_EVERY)
        {
            return;
        }
        let open = Down::Open {
            host: self.presence_name(),
        };
        let remote = &mut self.play.live.local.remote;
        remote.send_down(0, &open);
        remote.last_open = Some(Instant::now());
    }

    fn remote_host_receive(&mut self, from: u64, up: Up, cx: &mut Context<Self>) {
        let Some(run) = self
            .play
            .live
            .local
            .running
            .as_mut()
            .filter(|run| run.windowed && run.replay.is_none())
        else {
            if matches!(up, Up::Ask { .. }) {
                let no = Down::Ended("That session is over.".into());
                self.play.live.local.remote.send_down(from, &no);
            }
            return;
        };
        match run.remote.receive(&mut run.session, from, up) {
            Some(wire::Event::Offered(name)) => {
                push_row(
                    &mut self.output,
                    Feedback::Info(format!(
                        "{name} asks to join as a player: Accept it in the Server & Clients window"
                    )),
                );
                self.local_run_notify_tabs(cx);
            }
            Some(wire::Event::Left(player)) => {
                let name = run
                    .windows_name(player)
                    .unwrap_or_else(|| "A collaborator".into());
                push_row(
                    &mut self.output,
                    Feedback::Info(format!("{name} left the game")),
                );
                self.local_run_exit_player(player, cx);
                self.local_run_notify_tabs(cx);
            }
            Some(wire::Event::Purchase { player, bought })
                if run.prompts.front().is_some_and(|p| p.player == player) =>
            {
                if let Some(prompt) = run.prompts.pop_front() {
                    run.session.purchase_finished(&prompt, bought);
                }
            }
            Some(wire::Event::Purchase { .. }) => {}
            None => {}
        }
    }

    /// Tells every test window's tab row the offers and who plays from
    /// afar (the rows don't read this editor: it may be mid-update).
    fn local_run_notify_tabs(&self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_ref() else {
            return;
        };
        let offers: Vec<(u64, String)> = run
            .remote
            .offers
            .iter()
            .map(|o| (o.from, o.name.clone()))
            .collect();
        let afar: Vec<Ref> = run.remote.guests.iter().map(|g| g.player).collect();
        for window in &run.windows {
            let Some(shell) = window.shell.upgrade() else {
                continue;
            };
            let tabs = shell
                .read(cx)
                .play
                .live
                .local
                .peer
                .as_ref()
                .and_then(|link| link.tabs.upgrade());
            if let Some(tabs) = tabs {
                tabs.update(cx, |tabs, _| {
                    tabs.offers.clone_from(&offers);
                    tabs.afar.clone_from(&afar);
                });
            }
            shell.update(cx, |_, cx| cx.notify());
        }
    }

    /// An offer's Accept: the collaborator joins, in a tab of their own.
    pub(in crate::shell) fn local_run_accept_remote(&mut self, from: u64, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, move |host, cx| host.local_run_accept_remote(from, cx)) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let accepted = if run.session.clients.len() >= MAX_PLAYERS {
            run.remote.decline(from);
            Err(format!("The session is full ({MAX_PLAYERS} players)."))
        } else {
            run.remote.accept(&mut run.session, from)
        };
        let answer = match accepted {
            Ok((i, welcome)) => {
                let name = run.session.clients[i].name.clone();
                push_row(
                    &mut self.output,
                    Feedback::Info(format!("{name} joined from their own editor")),
                );
                let traffic = self.play.live.local.remote.traffic();
                self.play
                    .live
                    .local
                    .remote
                    .hosting
                    .get_or_insert((Instant::now(), traffic));
                welcome
            }
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("A collaborator couldn\u{2019}t join: {why}")),
                );
                Down::Ended(why)
            }
        };
        self.play.live.local.remote.send_down(from, &answer);
        self.local_run_open_player_windows(cx);
        self.local_run_notify_tabs(cx);
        cx.notify();
    }

    /// An offer's ✕.
    pub(in crate::shell) fn local_run_decline_remote(&mut self, from: u64, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, move |host, cx| host.local_run_decline_remote(from, cx)) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let no = run.remote.decline(from);
        self.play.live.local.remote.send_down(from, &no);
        self.local_run_notify_tabs(cx);
    }

    // --- The collaborator's side ---

    /// "Join as player" on a session someone announced: this editor's
    /// player (its avatar fetched off the UI thread) asks to join.
    pub(in crate::shell) fn remote_join(&mut self, host: u64, cx: &mut Context<Self>) {
        let remote = &self.play.live.local.remote;
        if remote.guest.is_some() || self.play.live.local.running.is_some() {
            return;
        }
        let Some(announced) = remote.hosts.iter().find(|h| h.from == host) else {
            return;
        };
        let host_name = announced.name.clone();
        self.play.live.local.remote.guest = Some(Guest {
            host,
            host_name: host_name.clone(),
            viewer: Viewer::default(),
            player: None,
            window: None,
            shell: WeakEntity::new_invalid(),
            core: None,
            frames: Vec::new(),
            sent: None,
            menu_open: false,
            since: Instant::now(),
            base: (0, 0),
        });
        let (own_name, own_id) = (self.presence_name(), self.presence_user_id().unwrap_or(0));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let ask = cx
                .background_executor()
                .spawn(async move {
                    let (player, _) = avatars::resolve();
                    let template = player
                        .character
                        .as_ref()
                        .and_then(|dom| rbx_binary::serialize(dom).ok());
                    // Signed out, the noob plays under the name the room
                    // knows them by.
                    let (name, user_id) = if player.user_id > 0 {
                        (player.name, player.user_id)
                    } else {
                        (own_name, own_id as i64)
                    };
                    (
                        Up::Ask {
                            name,
                            user_id,
                            template,
                        },
                        player.note,
                    )
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let (ask, note) = ask;
                let remote = &shell.play.live.local.remote;
                if remote.guest.as_ref().is_some_and(|g| g.host == host) {
                    remote.send_up(host, &ask);
                    push_row(
                        &mut shell.output,
                        Feedback::Info(format!(
                            "Asked {host_name} to join their Local Run ({note})"
                        )),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn remote_guest_receive(&mut self, from: u64, down: Down, cx: &mut Context<Self>) {
        let traffic = self.play.live.local.remote.traffic();
        let remote = &mut self.play.live.local.remote;
        if let Down::Open { host } = down {
            match remote.hosts.iter_mut().find(|h| h.from == from) {
                Some(known) => {
                    known.seen = Instant::now();
                    known.name = host;
                }
                None => {
                    push_row(
                        &mut self.output,
                        Feedback::Info(format!(
                            "{host} started a Local Run: Join as player from the Collaborators dock"
                        )),
                    );
                    self.play.live.local.remote.hosts.push(Announced {
                        from,
                        name: host,
                        seen: Instant::now(),
                    });
                    cx.notify();
                }
            }
            return;
        }
        let Some(guest) = remote.guest.as_mut().filter(|g| g.host == from) else {
            return;
        };
        match down {
            Down::Welcome { player, .. } => {
                guest.player = Some(player);
                guest.since = Instant::now();
                guest.base = traffic;
                cx.notify();
            }
            Down::Frame(frame) => guest.frames.push(*frame),
            Down::Ended(why) => self.remote_guest_end(&why, cx),
            Down::Open { .. } => {}
        }
    }

    /// The frames that came in shown in the player's window (opened with
    /// the first), and what the player did since sent up.
    fn remote_guest_play(&mut self, cx: &mut Context<Self>) {
        let Some(guest) = self.play.live.local.remote.guest.as_mut() else {
            return;
        };
        let Some(player) = guest.player else {
            return;
        };
        let frames = std::mem::take(&mut guest.frames);
        let reset = frames.iter().any(|f| f.reset);
        let mut changes: Vec<Change> = Vec::new();
        let mut shown = Frame::default();
        for frame in frames {
            changes = if frame.reset {
                guest.viewer.apply(&frame);
                Vec::new()
            } else {
                let mut all = changes;
                all.extend(guest.viewer.apply(&frame));
                all
            };
            if frame.core.is_some() {
                guest.core.clone_from(&frame.core);
            }
            shown.chat.extend(frame.chat);
            shown.notices.extend(frame.notices);
            shown.bursts.extend(frame.bursts);
            shown.rows.extend(frame.rows);
            shown.camera = frame.camera;
            shown.mouse = frame.mouse;
            shown.player_gui = frame.player_gui;
            shown.dialog = frame.dialog;
            shown.paused = frame.paused;
        }
        if guest.window.is_none() {
            if !reset {
                return;
            }
            self.remote_guest_open(player, cx);
        } else if reset {
            let dom = guest.viewer.dom_rc().borrow().clone();
            if let Some(shell) = guest.shell.upgrade() {
                shell.update(cx, |shell, cx| shell.local_run_resync(&dom, true, cx));
            }
        }
        let Some(guest) = self.play.live.local.remote.guest.as_mut() else {
            return;
        };
        let Some(shell) = guest.shell.upgrade() else {
            return;
        };
        let mut output = rbx_lua::Output::default();
        for (kind, line) in shown.rows {
            output.push_text(kind, line);
        }
        let view = Shown {
            dom: guest.viewer.dom_rc(),
            changes,
            input: true,
            client: true,
            camera: shown.camera,
            mouse: shown.mouse,
            player_gui: shown.player_gui,
            core: guest.core.clone(),
            chat: shown.chat,
            notices: shown.notices,
            dialog: shown.dialog,
            bursts: shown.bursts,
            rows: vec![(Peer::Client(0), output)],
            editables: false,
            paused: shown.paused,
        };
        let (actions, menu_open) =
            shell.update(cx, |shell, cx| shell.local_run_show_frame(view, cx));
        let (inputs, size, safe) = shell.update(cx, |shell, cx| {
            shell.viewport.update(cx, |viewport, _| {
                (
                    viewport.play.as_mut().map(|p| p.take()).unwrap_or_default(),
                    viewport.play_viewport_size(),
                    viewport.play_safe_insets(),
                )
            })
        });
        let mut up: Vec<Action> = Vec::new();
        if guest.sent != Some((size, safe)) {
            guest.sent = Some((size, safe));
            up.push(Action::Viewport(size));
            up.push(Action::SafeInsets(safe));
        }
        if guest.menu_open != menu_open {
            guest.menu_open = menu_open;
            up.push(Action::MenuOpen(menu_open));
        }
        up.extend(inputs.into_iter().map(Action::Input));
        let host = guest.host;
        let mut leave = false;
        let mut purchases = Vec::new();
        for action in actions {
            match action {
                CoreAction::Say(text) => up.push(Action::Chat(text)),
                CoreAction::Reset => up.push(Action::Reset),
                CoreAction::Purchase(bought) => purchases.push(bought),
                CoreAction::Leave => leave = true,
                CoreAction::Volume(_) => {}
            }
        }
        let remote = &self.play.live.local.remote;
        if !up.is_empty() {
            remote.send_up(host, &Up::Actions(up));
        }
        for bought in purchases {
            remote.send_up(host, &Up::Purchase(bought));
        }
        if leave {
            self.remote_guest_leave(cx);
        }
    }

    /// The player's window, a tab like a test player's on their copy of
    /// the client's DataModel.
    fn remote_guest_open(&mut self, player: Ref, cx: &mut Context<Self>) {
        let Some(guest) = self.play.live.local.remote.guest.as_ref() else {
            return;
        };
        let dom = guest.viewer.dom_rc().borrow().clone();
        let name = dom
            .get(player)
            .map_or_else(|| "Player".to_owned(), |p| p.name().to_owned());
        let title = SharedString::from(format!("{}\u{2019}s Local Run", guest.host_name));
        let host = cx.entity().downgrade();
        let link = PeerLink {
            host: host.clone(),
            home: Home::Player(player),
            view: Peer::Client(0),
            paused: false,
            tabs: WeakEntity::new_invalid(),
            tab_shown: true,
            remote: true,
        };
        let Some(build) = self.peer_shell(dom, &name, link) else {
            self.remote_guest_leave(cx);
            return;
        };
        let options = crate::window_options(&title, cx);
        let mut made = None;
        let opened = cx.open_window(options, |window, cx| {
            let shell = build(window, cx);
            let view = cx.new(|_| TestWindow::new(Home::Player(player), shell.clone()));
            shell.update(cx, |shell, _| {
                if let Some(link) = shell.play.live.local.peer.as_mut() {
                    link.tabs = view.downgrade();
                }
            });
            made = Some(shell.clone());
            window.on_window_should_close(cx, move |_, cx| {
                let host = host.clone();
                cx.defer(move |cx| {
                    let _ = host.update(cx, |host, cx| host.remote_guest_leave(cx));
                });
                false
            });
            cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
        });
        let (Ok(handle), Some(shell)) = (opened, made) else {
            self.remote_guest_leave(cx);
            return;
        };
        crate::menu_bar::retarget(&cx.entity(), cx);
        shell.update(cx, |shell, cx| {
            shell
                .viewport
                .update(cx, |viewport, _| viewport.set_play_local(true));
        });
        if let Some(guest) = self.play.live.local.remote.guest.as_mut() {
            guest.window = Some(handle.into());
            guest.shell = shell.downgrade();
        }
    }

    /// Leaves the session this editor plays (Exit, End Session, the
    /// window closing). False when it plays none.
    pub(in crate::shell) fn remote_guest_leave(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(guest) = self.play.live.local.remote.guest.as_ref() else {
            return false;
        };
        let host = guest.host;
        self.play.live.local.remote.send_up(host, &Up::Leave);
        self.remote_guest_end("You left the game.", cx);
        true
    }

    /// The session is over for this editor: its window closes, the Output
    /// says why and what crossed the wire.
    fn remote_guest_end(&mut self, why: &str, cx: &mut Context<Self>) {
        let (sent, received) = self.play.live.local.remote.traffic();
        let Some(guest) = self.play.live.local.remote.guest.take() else {
            return;
        };
        if let Some(window) = guest.window {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
        let seconds = guest.since.elapsed().as_secs_f64();
        let traffic = if guest.joined() {
            format!(
                " ({:.1} KB/s in, {:.2} KB/s out while playing)",
                rate(received.saturating_sub(guest.base.1), seconds),
                rate(sent.saturating_sub(guest.base.0), seconds)
            )
        } else {
            String::new()
        };
        push_row(
            &mut self.output,
            Feedback::Info(format!(
                "{}\u{2019}s Local Run: {why}{traffic}",
                guest.host_name
            )),
        );
        cx.notify();
    }
}

impl Shell {
    /// A collaborator's window on their player in someone else's session.
    pub(in crate::shell) fn local_run_remote_viewer(&self) -> bool {
        self.play.live.local.peer.as_ref().is_some_and(|p| p.remote)
    }

    /// The Collaborators dock's Local Run part: the sessions announced in
    /// the room, to join as a player, or the one this editor plays.
    pub(in crate::shell) fn remote_play_block(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !betas_on() {
            return None;
        }
        let remote = &self.play.live.local.remote;
        let line = |text: String| {
            div()
                .text_size(tokens::text_xs())
                .text_color(tokens::text2())
                .child(text)
        };
        let action = |id: (&'static str, usize), label: &'static str| {
            div()
                .id(id)
                .text_size(tokens::text_xs())
                .text_color(tokens::accent_line())
                .cursor_pointer()
                .child(label)
        };
        let rows: Vec<AnyElement> = match &remote.guest {
            Some(guest) => {
                let (text, label) = if guest.joined() {
                    (
                        format!("Playing {}\u{2019}s Local Run", guest.host_name),
                        "Leave",
                    )
                } else {
                    (
                        format!("Waiting for {} to let you in\u{2026}", guest.host_name),
                        "Cancel",
                    )
                };
                vec![v_flex()
                    .gap_1()
                    .child(line(text))
                    .child(
                        action(("remote-play-leave", 0), label).on_click(cx.listener(
                            |shell, _, _, cx| {
                                shell.remote_guest_leave(cx);
                            },
                        )),
                    )
                    .into_any_element()]
            }
            None => {
                let hosting = self.play.live.local.running.is_some();
                remote
                    .hosts
                    .iter()
                    .enumerate()
                    .map(|(i, host)| {
                        let from = host.from;
                        v_flex()
                            .gap_1()
                            .child(line(format!("{} is running a Local Run", host.name)))
                            .when(!hosting, |this| {
                                this.child(
                                    action(("remote-play-join", i), "Join as player").on_click(
                                        cx.listener(move |shell, _, _, cx| {
                                            shell.remote_join(from, cx)
                                        }),
                                    ),
                                )
                            })
                            .into_any_element()
                    })
                    .collect()
            }
        };
        if rows.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .m_2()
                .p_2()
                .gap_2()
                .border_1()
                .border_color(tokens::border())
                .rounded_sm()
                .child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_strong())
                        .child("Local Run"),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}

impl super::Running {
    /// The name of `player`'s tab.
    fn windows_name(&self, player: Ref) -> Option<String> {
        self.windows
            .iter()
            .find(|w| w.home == Home::Player(player))
            .map(|w| w.name.clone())
    }
}
