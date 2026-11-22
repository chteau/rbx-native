//! Test › Server & Clients, Studio's multi-client test: one session, its
//! server and each test player in a window of their own, while the window
//! that started it stays on the edited place.
//!
//! As in Studio (0.748, where each is a full Studio window titled
//! `Server` or after the place), every test window here is a whole editor
//! (`Shell`), its own viewport, Explorer, Properties and Output, built with
//! `Launch::test_peer` so it starts nothing of its own. They are views on
//! the session, never copies of it: the starting window owns the session
//! and its one frame loop, steps it, and hands each window its peer's
//! changes, camera, core GUI and printed lines ([`Shown`]); each player
//! window's keys and mouse go to that player's client alone, the window
//! that has focus being the only one that gets any. Edits made in a test
//! window go to its peer's DataModel, as a Play (local) edit does.
//!
//! Studio's controls: Pause / Step Forward, End Session (closes every
//! window), Add Clients, a player window's Exit (that player leaves) and
//! closing a window (a player's leaves, the Server's ends the session).
//! The Server window can show the first player's client (Current: Client),
//! as older Studio's server toggle did.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_dom::{Change, CFrameData, Ref, WeakDom};
use rbx_lua::server::ChatLine;
use rbx_sim::replay::Action;
use rbx_sim::{MouseBehavior, Peer, Session, Simulation};

use super::{
    changes_between, dedupe, editable_bindings, eye_and_target, mirror, pad_inputs, push_row,
    push_rows, structural, Running, MAX_PLAYERS,
};
use crate::command_bar::Feedback;
use crate::properties::Properties;
use crate::shell::Shell;
use crate::workspace_view::core_gui::CoreAction;

/// Which peer a test window was opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Home {
    Server,
    Player(Ref),
}

/// What a window is to a running Server & Clients session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// The window that started it, on the edited place.
    Edit,
    /// `client_view`: showing the first player's client.
    Server { client_view: bool },
    Player,
}

/// A test window, as the session's owner sees it.
pub(in crate::shell) struct PeerWindow {
    home: Home,
    name: String,
    shell: WeakEntity<Shell>,
    pub(super) handle: AnyWindowHandle,
    /// The Server window's Current: Client.
    client_view: bool,
    /// Its view switched: the next frame starts it over on its peer's
    /// DataModel as of that frame (changes pending at the switch are in
    /// the copy already).
    resync: bool,
    /// The player was kicked: the window keeps their last frame under the
    /// message.
    kicked: bool,
}

/// A test window's side: the session it shows.
pub(in crate::shell) struct PeerLink {
    host: WeakEntity<Shell>,
    home: Home,
    /// The peer shown now.
    view: Peer,
    pub(super) paused: bool,
}

impl PeerLink {
    /// The shown peer's DataModel, where an edit made here goes.
    pub(super) fn shown_dom(&self, cx: &App) -> Option<Rc<RefCell<WeakDom>>> {
        let host = self.host.upgrade()?;
        let run = host.read(cx).play.live.local.running.as_ref()?;
        run.session.sim(self.view).map(Simulation::dom_rc)
    }
}

/// One frame of a peer, for its window.
pub(super) struct Shown {
    dom: Rc<RefCell<WeakDom>>,
    changes: Vec<Change>,
    /// A player window: its client's core GUI and input.
    input: bool,
    client: bool,
    camera: Option<(CFrameData, f32)>,
    mouse: MouseBehavior,
    player_gui: Option<Ref>,
    core: Option<rbx_sim::gui::CoreGui>,
    chat: Vec<ChatLine>,
    notices: Vec<(String, String)>,
    dialog: Option<(String, String)>,
    bursts: Vec<(Ref, Option<u32>)>,
    rows: Vec<(Peer, rbx_lua::Output)>,
    editables: bool,
    paused: bool,
}

/// What `peer` printed, taken out of `output`.
fn take_rows(
    output: &mut Vec<(Peer, rbx_lua::Output)>,
    peer: Option<Peer>,
) -> Vec<(Peer, rbx_lua::Output)> {
    let (mine, rest) = std::mem::take(output)
        .into_iter()
        .partition(|(p, _)| Some(*p) == peer);
    *output = rest;
    mine
}

fn index_of(session: &Session, player: Ref) -> Option<usize> {
    session.clients.iter().position(|c| c.player == player)
}

/// A frame's view changes, kept per peer.
pub(super) fn collect(by_peer: &mut Vec<(Peer, Vec<Change>)>, peer: Peer, seen: Vec<Change>) {
    match by_peer.iter_mut().find(|(p, _)| *p == peer) {
        Some((_, changes)) => changes.extend(seen),
        None => by_peer.push((peer, seen)),
    }
}

/// `Emit` (`Some`) and `Clear` (`None`) calls the simulation made.
pub(super) fn bursts(sim: &mut Simulation) -> Vec<(Ref, Option<u32>)> {
    sim.take_bursts()
        .into_iter()
        .map(|(emitter, burst)| {
            let burst = match burst {
                rbx_sim::Burst::Emit(n) => Some(n),
                rbx_sim::Burst::Clear => None,
            };
            (emitter, burst)
        })
        .collect()
}

/// Each player window's keys, mouse and screen size to that player's
/// client; the Controller Emulator's pad to the player whose window last
/// had input.
pub(super) fn take_inputs(
    run: &mut Running,
    pad: Option<rbx_play::gamepad::PadState>,
    cx: &mut Context<Shell>,
) {
    for window in &run.windows {
        let Home::Player(player) = window.home else {
            continue;
        };
        let (Some(i), Some(shell)) = (index_of(&run.session, player), window.shell.upgrade())
        else {
            continue;
        };
        if window.kicked {
            continue;
        }
        let (inputs, size, safe) = shell.update(cx, |shell, cx| {
            shell.viewport.update(cx, |viewport, _| {
                (
                    viewport.play.as_mut().map(|p| p.take()).unwrap_or_default(),
                    viewport.play_viewport_size(),
                    viewport.play_safe_insets(),
                )
            })
        });
        run.session.act(Peer::Client(i), Action::Viewport(size));
        run.session.act(Peer::Client(i), Action::SafeInsets(safe));
        if !inputs.is_empty() {
            run.audible = Some(player);
        }
        for input in inputs {
            run.session.input(i, input);
        }
    }
    if let Some(i) = run.audible.and_then(|p| index_of(&run.session, p)) {
        for input in pad_inputs(run.pad, pad) {
            run.session.input(i, input);
        }
        run.pad = pad;
    }
}

impl Shell {
    /// Test › Server & Clients: the server and Test › Players' count of
    /// test players, each in a window of their own.
    pub(crate) fn server_and_clients(&mut self, cx: &mut Context<Self>) {
        let count = self.play.live.local.players;
        self.play_local_in(Some(count), cx);
    }

    /// What this window is to a running Server & Clients session.
    pub(crate) fn local_run_role(&self) -> Option<Role> {
        if let Some(peer) = &self.play.live.local.peer {
            return Some(match peer.home {
                Home::Server => Role::Server {
                    client_view: peer.view != Peer::Server,
                },
                Home::Player(_) => Role::Player,
            });
        }
        let run = self.play.live.local.running.as_ref()?;
        run.windowed.then_some(Role::Edit)
    }

    /// A test window's viewport frame: Studio's green for the server's
    /// view, blue for a client's.
    pub(crate) fn local_run_frame(&self) -> Option<Rgba> {
        let peer = self.play.live.local.peer.as_ref()?;
        Some(match peer.view {
            Peer::Server => rgb(0x01854b),
            Peer::Client(_) => rgb(0x335fff),
        })
    }

    /// In a test window, does `f` to the session's owner instead.
    pub(super) fn local_run_forward(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shell, &mut Context<Shell>) + 'static,
    ) -> bool {
        let Some(peer) = &self.play.live.local.peer else {
            return false;
        };
        // Later: the host updates this window's shell back (and End Session
        // closes it), which it can't while this update runs.
        let host = peer.host.clone();
        cx.defer(move |cx| {
            let _ = host.update(cx, f);
        });
        cx.notify();
        true
    }

    /// Opens the Server window and one per player, then shows each what
    /// its peer printed while joining.
    pub(super) fn local_run_open_windows(
        &mut self,
        output: Vec<(Peer, rbx_lua::Output)>,
        cx: &mut Context<Self>,
    ) {
        let players: Vec<Ref> = self
            .play
            .live
            .local
            .running
            .as_ref()
            .map(|run| run.session.clients.iter().map(|c| c.player).collect())
            .unwrap_or_default();
        let homes = std::iter::once(Home::Server).chain(players.into_iter().map(Home::Player));
        for home in homes {
            self.local_run_open_window(home, cx);
        }
        self.local_run_route_rows(output, cx);
    }

    fn local_run_open_window(&mut self, home: Home, cx: &mut Context<Self>) {
        let pose = self.viewport.read(cx).pose();
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let (peer, name) = match home {
            Home::Server => (Peer::Server, "Server".to_owned()),
            Home::Player(player) => {
                let Some(i) = index_of(&run.session, player) else {
                    return;
                };
                (Peer::Client(i), run.session.clients[i].name.clone())
            }
        };
        let Some(sim) = run.session.sim(peer) else {
            return;
        };
        let mut dom = sim.dom().clone();
        // The server's free camera starts where the edit view's is, as
        // Studio's Server window does.
        if home == Home::Server {
            if let Some(pose) = pose {
                crate::camera::write_pose(&mut dom, pose);
            }
        }
        dom.take_changes();
        let viewer = match rbx_viewer::Headless::from_dom_shared(&dom) {
            Ok(viewer) => viewer,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("The {name} window couldn\u{2019}t open: {why}")),
                );
                return;
            }
        };
        let cascade = run.windows.len() as f32 * 32.0;
        // Never saved: a test window's place is the running game's.
        let path = std::env::temp_dir()
            .join(format!("rbxnative-test-{}", std::process::id()))
            .join(format!("{name}.rbxl"));
        let place = crate::Place {
            explorer: crate::explorer::Explorer::from_dom(
                &dom,
                self.icon_pack,
                &self.folder_colors,
                &path,
            ),
            properties: Properties::new(self.database.clone()),
            selected: None,
            camera: crate::camera::PlaceCamera::from_dom(&dom),
            viewer,
            dom,
            database: self.database.clone(),
            path,
            format: self.format,
            folder_colors: self.folder_colors.clone(),
        };
        let boot = crate::fresh_boot();
        let settings = crate::settings::Settings {
            auto_recovery: false,
            ..boot.settings
        };
        let launch = crate::cli::Launch {
            test_peer: true,
            ..Default::default()
        };
        let user = boot.user;
        let host = cx.entity().downgrade();
        let link = PeerLink {
            host: host.clone(),
            home,
            view: peer,
            paused: false,
        };
        let title = SharedString::from(name.clone());
        let mut options = crate::window_options(&title, cx);
        if let Some(WindowBounds::Windowed(bounds)) = options.window_bounds.as_mut() {
            bounds.origin += point(px(cascade), px(cascade));
        }
        let badges = self.viewport.read(cx).core_gui.badges.clone();
        let mut made = None;
        let opened = cx.open_window(options, |window, cx| {
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(title, place, settings, launch, user, window, cx);
                shell.play.live.local.peer = Some(link);
                shell
            });
            made = Some(shell.downgrade());
            window.on_window_should_close(cx, move |_, cx| {
                let host = host.clone();
                cx.defer(move |cx| {
                    let _ = host.update(cx, |host, cx| host.local_run_window_closed(home, cx));
                });
                // The Server window closes with the session, at End Session.
                home != Home::Server
            });
            cx.new(|cx| Root::new(crate::window_blur::wrap(shell, cx), window, cx))
        });
        // Each new editor takes the menus; they stay this window's.
        crate::menu_bar::retarget(&cx.entity(), cx);
        let (Ok(handle), Some(shell)) = (opened, made) else {
            return;
        };
        if let Some(entity) = shell.upgrade() {
            entity.update(cx, |shell, cx| {
                shell.viewport.update(cx, |viewport, _| {
                    viewport.core_gui.badges = badges;
                    viewport.set_play_local(home != Home::Server);
                });
            });
        }
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.windows.push(PeerWindow {
                home,
                name,
                shell,
                handle: handle.into(),
                client_view: false,
                resync: false,
                kicked: false,
            });
        }
    }

    /// Each peer's printed lines to the window opened for it (the Server
    /// window keeps the server's whatever it shows, as each Studio window
    /// has its own process's Output).
    fn local_run_route_rows(
        &mut self,
        mut output: Vec<(Peer, rbx_lua::Output)>,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.play.live.local.running.as_ref() else {
            return;
        };
        for window in &run.windows {
            let home = match window.home {
                Home::Server => Some(Peer::Server),
                Home::Player(player) => index_of(&run.session, player).map(Peer::Client),
            };
            let rows = take_rows(&mut output, home);
            if let (false, Some(shell)) = (rows.is_empty(), window.shell.upgrade()) {
                shell.update(cx, |shell, cx| {
                    push_rows(&mut shell.output, rows, true);
                    cx.notify();
                });
            }
        }
    }

    /// One frame of the session to every test window: each its peer's
    /// changes, camera, core GUI and lines; then what each player did in
    /// their core GUI goes to their client.
    pub(super) fn local_run_present_windows(
        &mut self,
        by_peer: Vec<(Peer, Vec<Change>)>,
        kicked: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        for (name, message) in &kicked {
            push_row(
                &mut self.output,
                Feedback::Info(format!("{name} was kicked: {message}")),
            );
        }
        let generation = rbx_dom::editable::generation();
        let editables = generation != run.editables;
        run.editables = generation;
        let players: Vec<Ref> = run.session.clients.iter().map(|c| c.player).collect();
        let notices = run.session.take_notices();
        let mut output = run.session.take_output();
        let server_bursts = bursts(&mut run.session.server);
        let mut chat = Vec::new();
        let mut client_bursts = Vec::new();
        for client in &mut run.session.clients {
            chat.push(client.sim.take_chat());
            client_bursts.push(bursts(&mut client.sim));
        }
        // One client is heard: the one last played.
        let audible = run.audible.and_then(|p| players.iter().position(|q| *q == p));
        let mut heard = false;
        for (i, client) in run.session.clients.iter().enumerate() {
            let audio = client.sim.take_audio();
            if Some(i) == audible {
                run.sound.update(client.sim.voices(), audio, !run.paused);
                heard = true;
            }
        }
        if !heard {
            run.sound.update(Vec::new(), Vec::new(), false);
        }
        let mut done = Vec::new();
        let mut closed = Vec::new();
        for k in 0..run.windows.len() {
            let resync = std::mem::take(&mut run.windows[k].resync);
            let window = &run.windows[k];
            let Some(shell) = window.shell.upgrade() else {
                closed.push(k);
                continue;
            };
            if window.kicked {
                continue;
            }
            let (home, peer) = match window.home {
                Home::Server => {
                    let peer = match (window.client_view, players.is_empty()) {
                        (true, false) => Peer::Client(0),
                        _ => Peer::Server,
                    };
                    (Peer::Server, peer)
                }
                Home::Player(player) => match players.iter().position(|q| *q == player) {
                    Some(i) => (Peer::Client(i), Peer::Client(i)),
                    None => {
                        match kicked.iter().find(|(name, _)| *name == window.name) {
                            Some((_, message)) => {
                                let message = message.clone();
                                run.windows[k].kicked = true;
                                shell.update(cx, |shell, cx| shell.local_run_kicked(message, cx));
                            }
                            None => closed.push(k),
                        }
                        continue;
                    }
                },
            };
            let input = matches!(window.home, Home::Player(_));
            let player = match peer {
                Peer::Client(i) => Some(players[i]),
                Peer::Server => None,
            };
            let rows = take_rows(&mut output, Some(home));
            let dialog = run
                .prompts
                .front()
                .filter(|prompt| input && Some(prompt.player) == player)
                .map(|prompt| super::purchase_text(&run.session, prompt));
            let notices = notices
                .iter()
                .filter(|n| input && (n.player.is_none() || n.player == player))
                .map(|n| (n.title.clone(), n.text.clone()))
                .collect();
            let (chat, mut bursts) = match peer {
                Peer::Client(i) => (chat[i].clone(), client_bursts[i].clone()),
                Peer::Server => (Vec::new(), Vec::new()),
            };
            bursts.extend(server_bursts.iter().copied());
            let Some(sim) = run.session.sim_mut(peer) else {
                continue;
            };
            let client = player.is_some();
            let changes = if resync {
                let dom = sim.dom().clone();
                shell.update(cx, |shell, cx| {
                    if let Some(link) = shell.play.live.local.peer.as_mut() {
                        link.view = peer;
                    }
                    shell.local_run_resync(&dom, client, cx);
                });
                Vec::new()
            } else {
                by_peer
                    .iter()
                    .find(|(p, _)| *p == peer)
                    .map(|(_, c)| c.clone())
                    .unwrap_or_default()
            };
            let player_gui = player.and_then(|player| {
                let dom = sim.dom();
                dom.get(player)?
                    .children()
                    .iter()
                    .copied()
                    .find(|c| dom.get(*c).is_some_and(|i| i.class() == "PlayerGui"))
            });
            let shown = Shown {
                dom: sim.dom_rc(),
                changes,
                input,
                client,
                camera: if client { sim.camera() } else { None },
                mouse: sim.mouse_behavior(),
                player_gui,
                core: input.then(|| sim.core_gui()),
                chat,
                notices,
                dialog,
                bursts,
                rows,
                editables,
                paused: run.paused,
            };
            let (actions, menu_open) = shell.update(cx, |shell, cx| shell.local_run_show(shown, cx));
            if let (true, Some(player)) = (input, player) {
                done.push((player, actions, menu_open));
            }
        }
        let mut leaving = Vec::new();
        for (player, actions, menu_open) in done {
            let Some(i) = index_of(&run.session, player) else {
                continue;
            };
            let peer = Peer::Client(i);
            run.session.act(peer, Action::MenuOpen(menu_open));
            for action in actions {
                match action {
                    CoreAction::Say(text) => run.session.act(peer, Action::Chat(text)),
                    CoreAction::Reset => run.session.act(peer, Action::Reset),
                    CoreAction::Volume(volume) => run.sound.set_volume(volume),
                    CoreAction::Purchase(bought) => {
                        if let Some(prompt) = run.prompts.pop_front() {
                            run.session.purchase_finished(&prompt, bought);
                        }
                    }
                    CoreAction::Leave => leaving.push(player),
                }
            }
        }
        for k in closed.into_iter().rev() {
            let window = run.windows.remove(k);
            if let Home::Player(player) = window.home {
                if let Some(i) = index_of(&run.session, player) {
                    run.session.leave(i);
                }
            }
            let _ = window
                .handle
                .update(cx, |_, window, _| window.remove_window());
        }
        for player in leaving {
            self.local_run_exit_player(player, cx);
        }
        if !kicked.is_empty() {
            cx.notify();
        }
    }

    /// A test window shows one frame of its peer; what its player did in
    /// the core GUI, and whether its menu is open.
    fn local_run_show(&mut self, shown: Shown, cx: &mut Context<Self>) -> (Vec<CoreAction>, bool) {
        let Shown {
            dom,
            changes,
            input,
            client,
            camera,
            mouse,
            player_gui,
            core,
            chat,
            notices,
            dialog,
            bursts,
            rows,
            editables,
            paused,
        } = shown;
        let mut repaint = !rows.is_empty() || input;
        if let Some(peer) = self.play.live.local.peer.as_mut() {
            repaint |= peer.paused != paused;
            peer.paused = paused;
        }
        push_rows(&mut self.output, rows, true);
        let mut changes = changes;
        if editables {
            changes.extend(editable_bindings(&self.dom));
        }
        let changes = dedupe(changes);
        mirror(&dom.borrow(), &mut self.dom, &changes);
        self.dom.take_changes();
        if client {
            self.show_player_gui(player_gui, cx);
        }
        let taken = self.viewport.update(cx, |viewport, _| {
            viewport.particle_bursts(bursts);
            if let Some((cframe, fov)) = camera {
                let (eye, target) = eye_and_target(&cframe);
                viewport.look_at_with(eye, target, fov);
            }
            if !input {
                return (Vec::new(), false);
            }
            viewport.core_gui.push_chat(chat);
            let actions = viewport.core_gui.take_actions();
            // The prompt just answered stays closed; the next shows next frame.
            if !actions.iter().any(|a| matches!(a, CoreAction::Purchase(_))) {
                viewport.core_gui.purchase = dialog;
            }
            if let Some(mut core) = core {
                core.notifications.extend(notices);
                viewport.core_gui.update(core);
            }
            if let Some(play) = viewport.play.as_mut() {
                play.mouse = mouse;
            }
            (actions, viewport.core_gui.menu_open())
        });
        if !changes.is_empty() {
            repaint = true;
            let snapshots = self.dom.snapshot(&changes);
            let sent = changes.clone();
            self.viewport
                .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
            self.properties.dom_changed(&changes);
            if structural(&changes) {
                self.rebuild_explorer(cx);
            }
        }
        if repaint {
            cx.notify();
        }
        taken
    }

    /// A kicked player's window: their last frame under the message.
    fn local_run_kicked(&mut self, message: String, cx: &mut Context<Self>) {
        self.viewport.update(cx, |viewport, _| {
            viewport.play = None;
            viewport.core_gui.state = None;
            viewport.core_gui.kicked = Some(message);
        });
        cx.notify();
    }

    /// Shows `dom` (a peer's whole DataModel) instead of what this window
    /// showed.
    fn local_run_resync(&mut self, dom: &WeakDom, client: bool, cx: &mut Context<Self>) {
        self.deselect(cx);
        let changes = changes_between(&self.dom, dom);
        self.dom = dom.clone();
        self.dom.take_changes();
        if !client {
            self.show_player_gui(None, cx);
        }
        let snapshots = self.dom.snapshot(&changes);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, changes));
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        cx.notify();
    }

    /// The Server window's Current: Server / Client.
    pub(crate) fn local_run_toggle_server_view(&mut self, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, |host, cx| host.local_run_toggle_server_view(cx)) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if run.session.clients.is_empty() {
            return;
        }
        let Some(window) = run.windows.iter_mut().find(|w| w.home == Home::Server) else {
            return;
        };
        window.client_view = !window.client_view;
        window.resync = true;
    }

    /// Add Clients: one more test player, in a window of their own.
    pub(crate) fn local_run_add_client(&mut self, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, |host, cx| host.local_run_add_client(cx)) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if !run.windowed || run.session.clients.len() >= MAX_PLAYERS {
            return;
        }
        let template = run.player.as_ref().and_then(|p| p.character.clone());
        let joined = run.session.join_test_players(1, template.as_ref());
        let Ok([i]) = joined.as_deref() else {
            if let Err(why) = joined {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Add Clients failed: {why}")),
                );
            }
            return;
        };
        let client = &mut run.session.clients[*i];
        client.sim.watch_view_changes();
        let player = client.player;
        self.local_run_open_window(Home::Player(player), cx);
    }

    /// A player window's Exit: that player leaves and the window closes.
    pub(crate) fn local_run_exit_client(&mut self, cx: &mut Context<Self>) {
        let Some(Home::Player(player)) = self.play.live.local.peer.as_ref().map(|p| p.home) else {
            return;
        };
        self.local_run_forward(cx, move |host, cx| host.local_run_exit_player(player, cx));
    }

    fn local_run_exit_player(&mut self, player: Ref, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if let Some(i) = index_of(&run.session, player) {
            let name = run.session.clients[i].name.clone();
            run.session.leave(i);
            push_row(
                &mut self.output,
                Feedback::Info(format!("{name} left the game")),
            );
        }
        if let Some(k) = run
            .windows
            .iter()
            .position(|w| w.home == Home::Player(player))
        {
            let window = run.windows.remove(k);
            let _ = window
                .handle
                .update(cx, |_, window, _| window.remove_window());
        }
        cx.notify();
    }

    /// A test window was closed: a player's leaves, the Server's ends it.
    fn local_run_window_closed(&mut self, home: Home, cx: &mut Context<Self>) {
        match home {
            Home::Server => self.local_run_stop(cx),
            Home::Player(player) => self.local_run_exit_player(player, cx),
        }
    }
}
