//! Test › Server & Clients, Studio's multi-client test: one session, its
//! server and each test player in a tab of one test window (`tabs`), while
//! the window that started it stays on the edited place.
//!
//! Studio (0.748) opens a full Studio window per peer, titled `Server` or
//! after the place; here each tab is a whole editor (`Shell`), its own
//! viewport, Explorer, Properties and Output, built with
//! `Launch::test_peer` so it starts nothing of its own. They are views on
//! the session, never copies of it: the starting window owns the session
//! and its one frame loop, steps it, and hands each tab its peer's changes,
//! camera, core GUI and printed lines ([`Shown`]); the keys and mouse go to
//! the shown player tab's client alone. Edits made in a tab go to its
//! peer's DataModel, as a Play (local) edit does.
//!
//! Studio's controls: Pause / Step Forward, End Session (closes the test
//! window), Add Clients (a tab more), a player tab's Exit or close (that
//! player leaves); closing the test window ends the session. The Server tab
//! can show the first player's client (Current: Client), as older Studio's
//! server toggle did.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_dom::{CFrameData, Change, Ref, WeakDom};
use rbx_lua::server::ChatLine;
use rbx_sim::remote::Down;
use rbx_sim::replay::Action;
use rbx_sim::{MouseBehavior, Peer, Session, Simulation};

use super::remote;
use super::tabs::TestWindow;
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
    Server {
        client_view: bool,
    },
    Player,
}

/// The one test window holding every peer's tab.
pub(in crate::shell) struct TestWindowHandle {
    pub(super) handle: AnyWindowHandle,
    view: WeakEntity<TestWindow>,
}

impl TestWindowHandle {
    /// Takes `home`'s tab out of the window.
    fn remove(&self, home: Home, cx: &mut App) {
        let view = self.view.clone();
        let _ = self.handle.update(cx, |_, window, cx| {
            let _ = view.update(cx, |tabs, cx| tabs.remove(home, window, cx));
        });
    }

    /// The player the shown tab is, who alone gets the keys and mouse.
    fn input_target(&self, cx: &App) -> Option<Ref> {
        self.view.upgrade()?.read(cx).tabs().input_target()
    }
}

/// A peer's tab, as the session's owner sees it.
pub(in crate::shell) struct PeerWindow {
    pub(super) home: Home,
    pub(super) name: String,
    pub(super) shell: WeakEntity<Shell>,
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
    pub(super) host: WeakEntity<Shell>,
    pub(super) home: Home,
    /// The peer shown now.
    pub(super) view: Peer,
    pub(super) paused: bool,
    /// The test window this tab is in.
    pub(super) tabs: WeakEntity<TestWindow>,
    /// This tab is the one the test window shows.
    pub(in crate::shell) tab_shown: bool,
    /// A collaborator's view of their player in someone else's session
    /// (`remote`): `host` is this editor's own window, which holds the
    /// connection.
    pub(in crate::shell) remote: bool,
    /// The session's Remote inspector captures.
    pub(super) remotes: crate::shell::remote_inspector::Shared,
}

impl PeerLink {
    /// The window that started the session.
    pub(in crate::shell) fn host(&self) -> Option<Entity<Shell>> {
        self.host.upgrade()
    }

    /// The shown peer's DataModel, where an edit made here goes.
    pub(super) fn shown_dom(&self, cx: &App) -> Option<Rc<RefCell<WeakDom>>> {
        let host = self.host.upgrade()?;
        let run = host.read(cx).play.live.local.running.as_ref()?;
        run.session.sim(self.view).map(Simulation::dom_rc)
    }
}

/// One frame of a peer, for its window.
pub(super) struct Shown {
    pub(super) dom: Rc<RefCell<WeakDom>>,
    pub(super) changes: Vec<Change>,
    /// A player window: its client's core GUI and input.
    pub(super) input: bool,
    pub(super) client: bool,
    pub(super) camera: Option<(CFrameData, f32)>,
    pub(super) mouse: MouseBehavior,
    pub(super) player_gui: Option<Ref>,
    pub(super) core: Option<rbx_sim::gui::CoreGui>,
    pub(super) chat: Vec<ChatLine>,
    pub(super) notices: Vec<(String, String)>,
    pub(super) dialog: Option<(String, String)>,
    pub(super) bursts: Vec<(Ref, Option<u32>)>,
    pub(super) rows: Vec<(Peer, rbx_lua::Output)>,
    pub(super) editables: bool,
    pub(super) paused: bool,
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

/// The shown player tab's keys and mouse to that player's client, every
/// player's screen size to theirs; the Controller Emulator's pad to the
/// player whose tab last had input.
pub(super) fn take_inputs(
    run: &mut Running,
    pad: Option<rbx_play::gamepad::PadState>,
    cx: &mut Context<Shell>,
) {
    let target = run.test_window.as_ref().and_then(|t| t.input_target(cx));
    for window in &run.windows {
        let Home::Player(player) = window.home else {
            continue;
        };
        let (Some(i), Some(shell)) = (index_of(&run.session, player), window.shell.upgrade())
        else {
            continue;
        };
        // A collaborator's player takes their own editor's input.
        if window.kicked || run.remote.guest(player).is_some() {
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
        if target != Some(player) {
            continue;
        }
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
    /// test players, each in a tab of the test window.
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

    /// Opens the test window on the Server's tab, a tab per player already
    /// in, then shows each what its peer printed so far.
    pub(super) fn local_run_open_windows(
        &mut self,
        output: Vec<(Peer, rbx_lua::Output)>,
        cx: &mut Context<Self>,
    ) {
        self.local_run_open_window(Home::Server, cx);
        self.local_run_open_player_windows(cx);
        self.local_run_route_rows(output, cx);
    }

    /// A tab for each player who has none: the test players once they
    /// joined.
    pub(super) fn local_run_open_player_windows(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_ref() else {
            return;
        };
        let new: Vec<Ref> = run
            .session
            .clients
            .iter()
            .map(|c| c.player)
            .filter(|p| !run.windows.iter().any(|w| w.home == Home::Player(*p)))
            .collect();
        for player in new {
            self.local_run_open_window(Home::Player(player), cx);
        }
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
        let host = cx.entity().downgrade();
        let existing = run.test_window.as_ref().map(|t| (t.handle, t.view.clone()));
        let link = PeerLink {
            host: host.clone(),
            home,
            view: peer,
            paused: false,
            tabs: existing
                .as_ref()
                .map_or_else(WeakEntity::new_invalid, |(_, view)| view.clone()),
            tab_shown: existing.is_none(),
            remote: false,
            remotes: run.remotes.clone(),
        };
        let Some(build) = self.peer_shell(dom, &name, link) else {
            return;
        };
        let badges = self.viewport.read(cx).core_gui.badges.clone();
        let (handle, shell) = match existing {
            Some((handle, view)) => {
                let made = handle.update(cx, |_, window, cx| {
                    let shell = build(window, cx);
                    let _ = view.update(cx, |tabs, cx| tabs.add(home, shell.clone(), cx));
                    shell
                });
                let Ok(shell) = made else {
                    return;
                };
                (None, shell)
            }
            None => {
                let options = crate::window_options(&SharedString::from("Server & Clients"), cx);
                let mut made = None;
                let opened = cx.open_window(options, |window, cx| {
                    let shell = build(window, cx);
                    let view = cx.new(|_| TestWindow::new(home, shell.clone()));
                    shell.update(cx, |shell, _| {
                        if let Some(link) = shell.play.live.local.peer.as_mut() {
                            link.tabs = view.downgrade();
                        }
                    });
                    made = Some((shell, view.downgrade()));
                    window.on_window_should_close(cx, move |_, cx| {
                        let host = host.clone();
                        cx.defer(move |cx| {
                            let _ = host.update(cx, |host, cx| host.local_run_stop(cx));
                        });
                        // It closes with the session, at End Session.
                        false
                    });
                    cx.new(|cx| Root::new(crate::window_blur::wrap(view, cx), window, cx))
                });
                let (Ok(handle), Some((shell, view))) = (opened, made) else {
                    return;
                };
                (
                    Some(TestWindowHandle {
                        handle: handle.into(),
                        view,
                    }),
                    shell,
                )
            }
        };
        // Each new editor takes the menus; they stay this window's.
        crate::menu_bar::retarget(&cx.entity(), cx);
        shell.update(cx, |shell, cx| {
            shell.viewport.update(cx, |viewport, _| {
                viewport.core_gui.badges = badges;
                viewport.set_play_local(home != Home::Server);
            });
        });
        if let Some(run) = self.play.live.local.running.as_mut() {
            if handle.is_some() {
                run.test_window = handle;
            }
            run.windows.push(PeerWindow {
                home,
                name,
                shell: shell.downgrade(),
                client_view: false,
                resync: false,
                kicked: false,
            });
        }
    }

    /// What builds a test tab's editor on `dom` (a peer's DataModel, a
    /// place nobody saves), linked by `link`; it starts nothing of its own.
    /// `None` when its viewer can't be made (said in the Output).
    pub(super) fn peer_shell(
        &mut self,
        mut dom: WeakDom,
        name: &str,
        link: PeerLink,
    ) -> Option<impl FnOnce(&mut Window, &mut App) -> Entity<Shell> + 'static> {
        dom.take_changes();
        let viewer = match rbx_viewer::Headless::from_dom_shared(&dom) {
            Ok(viewer) => viewer,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("The {name} window couldn\u{2019}t open: {why}")),
                );
                return None;
            }
        };
        // Never saved: a test tab's place is the running game's.
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
        let title = SharedString::from(name.to_owned());
        Some(move |window: &mut Window, cx: &mut App| {
            cx.new(|cx| {
                let mut shell = Shell::new(title, place, settings, launch, user, window, cx);
                shell.play.live.local.peer = Some(link);
                shell
            })
        })
    }

    /// The test window showed this tab (`shown`) or another: a tab put
    /// away lets go of every key and button its player held, so nothing
    /// stays down on a client nobody is looking at; the shown one takes the
    /// keyboard, in its viewport on a player's tab.
    pub(in crate::shell) fn local_run_tab_shown(
        &mut self,
        shown: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(link) = self.play.live.local.peer.as_mut() else {
            return;
        };
        link.tab_shown = shown;
        let player = matches!(link.home, Home::Player(_));
        if !shown {
            self.viewport.update(cx, |viewport, _| {
                if let Some(play) = viewport.play.as_mut() {
                    play.release();
                }
            });
            return;
        }
        let focus = if player {
            self.viewport.read(cx).focus_handle()
        } else {
            self.root_focus.clone()
        };
        window.focus(&focus, cx);
        cx.notify();
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
        let audible = run
            .audible
            .and_then(|p| players.iter().position(|q| *q == p));
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
        // Frames for collaborators' editors, and goodbyes.
        let mut outgoing = Vec::new();
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
                                if let Some(to) = run.remote.drop_player(player) {
                                    outgoing.push((to, Down::Ended(message.clone())));
                                }
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
            let remote = player.filter(|p| input && run.remote.guest(*p).is_some());
            if let Some(player) = remote {
                let frame = remote::frame_of(&shown);
                let dom = shown.dom.borrow();
                outgoing.extend(run.remote.frame(player, &dom, &shown.changes, frame));
            }
            let (actions, menu_open) =
                shell.update(cx, |shell, cx| shell.local_run_show_frame(shown, cx));
            // What the host does in a collaborator's tab stays there: their
            // own editor plays them.
            if let (true, Some(player), None) = (input, player, remote) {
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
                if let Some(to) = run.remote.drop_player(player) {
                    outgoing.push((to, Down::Ended("You left the game.".into())));
                }
                if let Some(i) = index_of(&run.session, player) {
                    run.session.leave(i);
                }
            }
            if let Some(test_window) = &run.test_window {
                test_window.remove(window.home, cx);
            }
        }
        for (to, message) in outgoing {
            self.play.live.local.remote.send_down(to, &message);
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
    pub(super) fn local_run_show_frame(
        &mut self,
        shown: Shown,
        cx: &mut Context<Self>,
    ) -> (Vec<CoreAction>, bool) {
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
    pub(super) fn local_run_kicked(&mut self, message: String, cx: &mut Context<Self>) {
        self.viewport.update(cx, |viewport, _| {
            viewport.play = None;
            viewport.core_gui.state = None;
            viewport.core_gui.kicked = Some(message);
        });
        cx.notify();
    }

    /// Shows `dom` (a peer's whole DataModel) instead of what this window
    /// showed.
    pub(super) fn local_run_resync(&mut self, dom: &WeakDom, client: bool, cx: &mut Context<Self>) {
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

    /// Add Clients: one more test player, in a tab of their own.
    pub(crate) fn local_run_add_client(&mut self, cx: &mut Context<Self>) {
        if !crate::beta::enabled(super::BETA_ID) {
            return;
        }
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

    /// A player tab's Exit (or its close): that player leaves and the tab
    /// closes.
    pub(crate) fn local_run_exit_client(&mut self, cx: &mut Context<Self>) {
        let Some(Home::Player(player)) = self.play.live.local.peer.as_ref().map(|p| p.home) else {
            return;
        };
        self.local_run_close_tab(player, cx);
    }

    /// `player`'s tab closed (its ×, a middle click): they leave.
    pub(in crate::shell) fn local_run_close_tab(&mut self, player: Ref, cx: &mut Context<Self>) {
        self.local_run_forward(cx, move |host, cx| host.local_run_exit_player(player, cx));
    }

    pub(super) fn local_run_exit_player(&mut self, player: Ref, cx: &mut Context<Self>) {
        if self.remote_guest_leave(cx) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if let Some(to) = run.remote.drop_player(player) {
            let bye = Down::Ended("The host took you out of the game.".into());
            self.play.live.local.remote.send_down(to, &bye);
        }
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
            if let Some(test_window) = &run.test_window {
                test_window.remove(window.home, cx);
            }
        }
        cx.notify();
    }
}
