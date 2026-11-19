//! Model › Run (local): the place runs live on `rbx_sim`, off a copy of it.
//! The 3D viewport, Explorer and Properties show the simulation moving, the
//! Output streams what scripts print, Pause holds it and Stop puts the
//! edited place back exactly as it was. Behind its beta switch.
//!
//! Play (local) is the same run with a local player: their character (see
//! `avatars`) walks on WASD, jumps on Space, and the camera follows it,
//! turned by right-drag and zoomed by the wheel.
//!
//! While it runs `Shell::dom` holds a mirror of the simulation's DOM and the
//! edited place sits in the run's stash (see `play::live` for the same swap
//! during Play). Each frame's changes are copied from the simulation's DOM
//! into the mirror; edits made to the mirror are copied the other way into
//! the simulation, logged, so it routes them on its next step.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use rbx_dom::{Change, Ref, WeakDom};

use super::output::game::Context as OutputContext;
use super::play::Phase;
use super::Shell;

mod avatars;
mod sound;
use crate::command_bar::Feedback;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;
use crate::workspace_view::core_gui::CoreAction;

pub(crate) const BETA_ID: &str = "local-run";
const FRAME_HZ: f64 = 60.0;
/// Frames one tick may step, so a slow frame never spirals.
const MAX_FRAMES_PER_TICK: u64 = 4;
const TICK: Duration = Duration::from_millis(8);

/// The edited place's state while the simulation's copy is shown.
struct Stash {
    dom: WeakDom,
    history: History,
    selection: Vec<Ref>,
}

struct Running {
    session: rbx_sim::Session,
    /// Which peer's DataModel the Explorer, Properties and viewport show;
    /// a client view gets the keys and moves the camera.
    view: rbx_sim::Peer,
    stash: Stash,
    paused: bool,
    /// Step while paused: the next tick runs exactly one frame.
    step_once: bool,
    /// Wall seconds the run has been unpaused for.
    clock: f64,
    last: Instant,
    /// Dropping it stops the frame loop.
    task: Option<Task<()>>,
    /// Play (local): a local player whose character the keys drive.
    playing: bool,
    /// What the viewed peer hears.
    sound: sound::Mixer,
}

pub(crate) struct LocalRun {
    running: Option<Running>,
    /// Play (local) is fetching the avatar.
    pub(in crate::shell) starting: bool,
    /// Test › Players: how many local players Play (local) starts with.
    pub(in crate::shell) players: usize,
}

impl Default for LocalRun {
    fn default() -> Self {
        LocalRun {
            running: None,
            starting: false,
            players: 1,
        }
    }
}

/// The most local players Play (local) runs.
pub(crate) const MAX_PLAYERS: usize = 4;

/// Animations (the default ones, the catalog's) from Roblox's asset
/// delivery; they land in the asset cache for the next run.
struct Fetcher(rbx_cloud::Client);

impl rbx_assets::AssetFetcher for Fetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.0
            .asset(id)
            .map(|content| content.bytes)
            .map_err(|err| rbx_assets::FetchError::Other {
                id,
                message: err.to_string(),
            })
    }
}

impl LocalRun {
    pub(in crate::shell) fn active(&self) -> bool {
        self.running.is_some()
    }

    pub(in crate::shell) fn paused(&self) -> bool {
        self.running.as_ref().is_some_and(|run| run.paused)
    }

    /// The edited place, while the simulation's copy is shown.
    pub(in crate::shell) fn edited(&self) -> Option<&WeakDom> {
        self.running.as_ref().map(|run| &run.stash.dom)
    }
}

/// Puts `running` into `dom`'s place and returns the edited place it replaced.
fn stash_edited(dom: &mut WeakDom, running: WeakDom) -> WeakDom {
    std::mem::replace(dom, running)
}

/// Puts the edited place back, with a clean change log.
fn restore_edited(dom: &mut WeakDom, edited: WeakDom) {
    *dom = edited;
    dom.take_changes();
}

/// Copies `changes` from `from` into `into`: a property by value (or its
/// removal), an instance with its subtree and referents, a removal, a move,
/// a class. Writes are logged in `into`.
fn mirror(from: &WeakDom, into: &mut WeakDom, changes: &[Change]) {
    for change in changes {
        match change {
            Change::Property { referent, name } => {
                let (Some(source), true) = (from.get(*referent), into.get(*referent).is_some())
                else {
                    continue;
                };
                if name == "Name" {
                    let _ = into.set_name(*referent, source.name());
                } else if let Some(value) = source.properties().get(name.as_str()) {
                    let _ = into.set_property(*referent, name, value.clone());
                } else {
                    let _ = into.remove_property(*referent, name);
                }
            }
            // A parentless instance (an AnimationTrack, a script's
            // `Instance.new` not parented yet, an old character) isn't in
            // the game's tree, so it stays out of the shown one.
            Change::Added(referent) if from.parent(*referent).is_none() => {}
            Change::Added(referent) => copy_subtree(from, into, *referent),
            Change::Parent { referent, .. } if from.parent(*referent).is_none() => {
                into.remove(*referent);
            }
            Change::Parent { referent, .. } => {
                if from.get(*referent).is_none() {
                    continue;
                }
                if into.get(*referent).is_none() {
                    copy_subtree(from, into, *referent);
                } else {
                    place(from, into, *referent);
                }
            }
            Change::Removed(referent) => {
                into.remove(*referent);
            }
            Change::Class(referent) => {
                if let (Some(source), true) = (from.get(*referent), into.get(*referent).is_some()) {
                    let _ = into.set_class(*referent, source.class());
                }
            }
        }
    }
}

/// Inserts `root` and everything under it that `into` lacks, parents as in
/// `from`.
fn copy_subtree(from: &WeakDom, into: &mut WeakDom, root: Ref) {
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        let Some(source) = from.get(at) else {
            continue;
        };
        stack.extend(source.children().iter().rev().copied());
        if into.get(at).is_some() {
            continue;
        }
        let mut copy = rbx_dom::Instance::new(at, source.class(), source.name());
        *copy.properties_mut() = source.properties().clone();
        into.insert(copy);
        place(from, into, at);
    }
}

/// Gives `referent` in `into` the parent it has in `from`.
fn place(from: &WeakDom, into: &mut WeakDom, referent: Ref) {
    match from.parent(referent) {
        Some(parent) if into.get(parent).is_some() => into.set_parent(referent, Some(parent)),
        Some(_) => {}
        None if from.root_refs().contains(&referent) => into.set_parent(referent, None),
        None => into.unparent(referent),
    }
}

/// Every instance under `dom`'s roots, parents before children.
fn all_refs(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(at) = stack.pop() {
        if let Some(instance) = dom.get(at) {
            out.push(at);
            stack.extend(instance.children().iter().rev().copied());
        }
    }
    out
}

/// The changes that turn `from` into `to`, matched by referent: what the
/// viewport needs to go back from the run to the edited place.
fn changes_between(from: &WeakDom, to: &WeakDom) -> Vec<Change> {
    let mut changes = Vec::new();
    for referent in all_refs(from) {
        if to.get(referent).is_none() {
            changes.push(Change::Removed(referent));
        }
    }
    for referent in all_refs(to) {
        let Some(after) = to.get(referent) else {
            continue;
        };
        let Some(before) = from.get(referent) else {
            changes.push(Change::Added(referent));
            continue;
        };
        if from.parent(referent) != to.parent(referent) {
            changes.push(Change::Parent {
                referent,
                old: from.parent(referent),
                new: to.parent(referent),
            });
        }
        if before.class() != after.class() {
            changes.push(Change::Class(referent));
        }
        if before.name() != after.name() {
            changes.push(Change::Property {
                referent,
                name: "Name".to_owned(),
            });
        }
        let names: HashSet<&str> = before
            .properties()
            .keys()
            .chain(after.properties().keys())
            .map(|name| name.as_ref())
            .collect();
        for name in names {
            if before.properties().get(name) != after.properties().get(name) {
                changes.push(Change::Property {
                    referent,
                    name: name.to_owned(),
                });
            }
        }
    }
    changes
}

/// `changes` without a repeated write to the same property.
fn dedupe(changes: Vec<Change>) -> Vec<Change> {
    let mut seen: HashSet<(Ref, String)> = HashSet::new();
    changes
        .into_iter()
        .filter(|change| match change {
            Change::Property { referent, name } => seen.insert((*referent, name.clone())),
            _ => true,
        })
        .collect()
}

fn structural(changes: &[Change]) -> bool {
    changes.iter().any(|change| match change {
        Change::Property { name, .. } => name == "Name",
        _ => true,
    })
}

/// What each side of the run printed, as game rows like real Play's:
/// tagged Server or Client, or Local Run for a run with no players.
fn push_rows(
    log: &mut super::output::OutputLog,
    outputs: Vec<(rbx_sim::Peer, rbx_lua::Output)>,
    playing: bool,
) {
    for (peer, output) in outputs {
        let context = match peer {
            _ if !playing => OutputContext::LocalRun,
            rbx_sim::Peer::Server => OutputContext::Server,
            rbx_sim::Peer::Client(_) => OutputContext::Client,
        };
        for row in rows_of(&output) {
            log.push_game(context, row, Vec::new(), None);
        }
    }
}

/// The name a view goes by: Server, or the player's.
pub(crate) fn view_label(session: &rbx_sim::Session, view: rbx_sim::Peer) -> String {
    match view {
        rbx_sim::Peer::Server => "Server".to_owned(),
        rbx_sim::Peer::Client(i) => session
            .clients
            .get(i)
            .map_or_else(|| format!("Player{}", i + 1), |c| c.name.clone()),
    }
}

/// One row of the run's own, tagged "Local Run" like a game row.
fn push_row(log: &mut super::output::OutputLog, row: Feedback) {
    log.push_game(OutputContext::LocalRun, row, Vec::new(), None);
}

fn rows_of(output: &rbx_lua::Output) -> Vec<Feedback> {
    output
        .lines()
        .iter()
        .zip(output.kinds())
        .map(|(line, kind)| match kind {
            rbx_lua::LineKind::Print => Feedback::Output(line.clone()),
            rbx_lua::LineKind::Warn => Feedback::Warning(line.clone()),
            rbx_lua::LineKind::Error => Feedback::Error(line.clone()),
        })
        .collect()
}

impl Shell {
    fn local_run_can_start(&self) -> bool {
        crate::beta::enabled(BETA_ID)
            && !self.local_run_active()
            && !self.play.live.local.starting
            && self.play.phase == Phase::Idle
    }

    pub(crate) fn run_local(&mut self, cx: &mut Context<Self>) {
        if self.local_run_can_start() {
            self.start_local(None, cx);
        }
    }

    /// Play (local): works out the player off the UI thread, then starts.
    pub(crate) fn play_local(&mut self, cx: &mut Context<Self>) {
        if !self.local_run_can_start() {
            return;
        }
        self.play.live.local.starting = true;
        push_row(
            &mut self.output,
            Feedback::Info("Play (local): getting your avatar\u{2026}".to_owned()),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let player = cx
                .background_executor()
                .spawn(async { avatars::resolve() })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.play.live.local.starting = false;
                if shell.local_run_can_start() {
                    shell.start_local(Some(player), cx);
                }
            });
        })
        .detach();
    }

    fn start_local(
        &mut self,
        player: Option<(avatars::Player, avatars::StudioAvatars)>,
        cx: &mut Context<Self>,
    ) {
        let mut session = match rbx_sim::Session::new(self.dom.clone()) {
            Ok(session) => session,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Local Run couldn\u{2019}t start: {why}")),
                );
                cx.notify();
                return;
            }
        };
        session.set_fetcher(Arc::new(Fetcher(rbx_cloud::Client::new(
            rbx_cloud::ApiKey::from_env_or_config(),
        ))));
        session.start();
        let playing = player.is_some();
        let mut note = None;
        if let Some((player, avatars)) = player {
            session.server.set_avatars(Box::new(avatars));
            // You, then the extra test players: noobs, numbered as Studio
            // numbers its test players.
            let mut joined = session
                .join(&player.name, player.user_id, player.character)
                .map(|_| ());
            for n in 2..=self.play.live.local.players {
                if joined.is_ok() {
                    joined = session
                        .join(&format!("Player{n}"), -(n as i64), None)
                        .map(|_| ());
                }
            }
            if let Err(why) = joined {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Play (local) couldn\u{2019}t start: {why}")),
                );
                cx.notify();
                return;
            }
            note = Some(player.note);
        }
        let view = if playing {
            rbx_sim::Peer::Client(0)
        } else {
            rbx_sim::Peer::Server
        };
        for client in &mut session.clients {
            client.sim.watch_view_changes();
        }
        session.server.watch_view_changes();
        let mut shown = session
            .sim(view)
            .map(|s| s.dom().clone())
            .unwrap_or_default();
        shown.take_changes();
        let output = session.take_output();
        let history = std::mem::replace(&mut self.history, History::new(DEFAULT_CAP));
        let selection = self.selected_all().to_vec();
        self.deselect(cx);
        let dom = stash_edited(&mut self.dom, shown);
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this
                .update(cx, |shell, cx| shell.local_run_tick(cx))
                .is_err()
            {
                break;
            }
        });
        self.play.live.local.running = Some(Running {
            session,
            view,
            stash: Stash {
                dom,
                history,
                selection,
            },
            paused: false,
            step_once: false,
            clock: 0.0,
            last: Instant::now(),
            task: Some(task),
            playing,
            sound: sound::Mixer::new(self.viewport.read(cx).core_gui.volume),
        });
        if playing {
            self.viewport
                .update(cx, |viewport, _| viewport.set_play_local(true));
        }
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        let started = if playing {
            "Play (local) started"
        } else {
            "Local Run started"
        };
        push_row(&mut self.output, Feedback::Info(started.to_owned()));
        if let Some(note) = note {
            push_row(&mut self.output, Feedback::Info(note));
        }
        push_rows(&mut self.output, output, playing);
        cx.notify();
    }

    /// The view the run shows, while one runs.
    pub(crate) fn local_run_view(&self) -> Option<(rbx_sim::Peer, String)> {
        let run = self.play.live.local.running.as_ref()?;
        run.playing
            .then(|| (run.view, view_label(&run.session, run.view)))
    }

    /// Test › View during Play (local): the next of the server and each
    /// player's client. The shown tree becomes that peer's.
    pub(crate) fn local_run_next_view(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let count = run.session.clients.len();
        run.view = match run.view {
            rbx_sim::Peer::Server if count > 0 => rbx_sim::Peer::Client(0),
            rbx_sim::Peer::Client(i) if i + 1 < count => rbx_sim::Peer::Client(i + 1),
            _ => rbx_sim::Peer::Server,
        };
        let client = matches!(run.view, rbx_sim::Peer::Client(_));
        let Some(sim) = run.session.sim_mut(run.view) else {
            return;
        };
        sim.take_view_changes();
        if !client {
            rbx_viewer::set_player_gui(None);
        }
        let shown = sim.dom().clone();
        self.deselect(cx);
        let changes = changes_between(&self.dom, &shown);
        self.dom = shown;
        self.dom.take_changes();
        let snapshots = self.dom.snapshot(&changes);
        self.viewport.update(cx, |viewport, _| {
            viewport.apply_changes(snapshots, changes);
            viewport.set_play_local(client);
        });
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        cx.notify();
    }

    /// Draws `player_gui`'s screens (a `PlayerGui` in the shown tree)
    /// instead of `StarterGui`'s, or (`None`) the edit view's again; the
    /// viewport re-plans its GUI when that changes.
    fn show_player_gui(&mut self, player_gui: Option<rbx_dom::Ref>, cx: &mut Context<Self>) {
        if rbx_viewer::player_gui() == player_gui {
            return;
        }
        rbx_viewer::set_player_gui(player_gui);
        // A reparenting nobody can place re-plans every screen.
        let Some(any) = player_gui.or_else(|| self.dom.root_refs().first().copied()) else {
            return;
        };
        let parent = self.dom.parent(any);
        let changes = vec![Change::Parent {
            referent: any,
            old: parent,
            new: parent,
        }];
        let snapshots = self.dom.snapshot(&changes);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, changes));
    }

    /// Test › Players during Play (local): the last player leaves (their
    /// `PlayerRemoving` runs), as a player closing the game does.
    pub(crate) fn local_run_remove_player(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let count = run.session.clients.len();
        if count < 2 {
            return;
        }
        let name = run.session.clients[count - 1].name.clone();
        if run.view == rbx_sim::Peer::Client(count - 1) {
            self.local_run_next_view(cx);
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        run.session.leave(count - 1);
        push_row(
            &mut self.output,
            Feedback::Info(format!("{name} left the game")),
        );
        cx.notify();
    }

    /// How many local players the running game has.
    pub(crate) fn local_run_player_count(&self) -> Option<usize> {
        let run = self.play.live.local.running.as_ref()?;
        run.playing.then_some(run.session.clients.len())
    }

    /// Test › Players: 1 to [`MAX_PLAYERS`], for the next Play (local).
    pub(crate) fn local_run_next_players(&mut self, cx: &mut Context<Self>) {
        let local = &mut self.play.live.local;
        local.players = local.players % MAX_PLAYERS + 1;
        cx.notify();
    }

    pub(crate) fn local_run_active(&self) -> bool {
        self.play.live.local.active()
    }

    pub(crate) fn local_run_paused(&self) -> bool {
        self.play.live.local.paused()
    }

    pub(crate) fn local_run_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.paused = !run.paused;
            cx.notify();
        }
    }

    /// Step while paused: one frame of the game, shown as a tick shows it.
    pub(crate) fn local_run_step_frame(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if !run.paused {
            return;
        }
        run.step_once = true;
        run.paused = false;
        self.local_run_tick(cx);
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.paused = true;
            run.step_once = false;
        }
        cx.notify();
    }

    pub(crate) fn local_run_stop(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.take() else {
            return;
        };
        let Running {
            mut session,
            stash,
            task,
            playing,
            ..
        } = run;
        drop(task);
        self.viewport
            .update(cx, |viewport, _| viewport.set_play_local(false));
        rbx_viewer::set_player_gui(None);
        let output = session.take_output();
        let seconds = session.time();
        drop(session);
        self.deselect(cx);
        let back = changes_between(&self.dom, &stash.dom);
        restore_edited(&mut self.dom, stash.dom);
        let snapshots = self.dom.snapshot(&back);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, back));
        self.history = stash.history;
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        self.reselect(stash.selection, cx);
        push_rows(&mut self.output, output, playing);
        push_row(
            &mut self.output,
            Feedback::Info(format!(
                "Local Run stopped after {seconds:.1} s of game time"
            )),
        );
        cx.notify();
    }

    /// One tick of the frame loop: steps the frames due on the wall clock.
    fn local_run_tick(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let now = Instant::now();
        let elapsed = now.duration_since(run.last).as_secs_f64();
        run.last = now;
        if run.paused {
            run.sound.update(Vec::new(), false);
            return;
        }
        run.clock += elapsed;
        if run.step_once {
            run.clock = run.session.time() + 1.0 / FRAME_HZ;
        }
        let due =
            ((run.clock * FRAME_HZ).floor() as u64).saturating_sub(run.session.server.frame());
        if due == 0 {
            return;
        }
        let steps = due.min(MAX_FRAMES_PER_TICK);
        let viewing_client = matches!(run.view, rbx_sim::Peer::Client(_));
        if run.playing && viewing_client {
            let (inputs, size) = self.viewport.update(cx, |viewport, _| {
                (
                    viewport.play.as_mut().map(|p| p.take()),
                    viewport.play_viewport_size(),
                )
            });
            if let rbx_sim::Peer::Client(i) = run.view {
                if let Some(client) = run.session.clients.get_mut(i) {
                    client.sim.set_viewport(size);
                }
                for input in inputs.unwrap_or_default() {
                    run.session.input(i, input);
                }
            }
        }
        let viewed_player = match run.view {
            rbx_sim::Peer::Client(i) => run.session.clients.get(i).map(|c| c.player),
            rbx_sim::Peer::Server => None,
        };
        let kicked_before = run.session.kicked.len();
        let mut changes = Vec::new();
        for _ in 0..steps {
            run.session.step();
            for peer in std::iter::once(rbx_sim::Peer::Server)
                .chain((0..run.session.clients.len()).map(rbx_sim::Peer::Client))
            {
                let Some(sim) = run.session.sim_mut(peer) else {
                    continue;
                };
                let seen = sim.take_view_changes();
                if peer == run.view {
                    changes.extend(seen);
                }
            }
        }
        if due > steps {
            run.clock = run.session.time();
        }
        // Someone kicked: the Output says so; the viewed player's own view
        // keeps their last frame under the message, the run goes on.
        let kicked: Vec<(String, String)> = run.session.kicked[kicked_before..].to_vec();
        for (name, message) in &kicked {
            push_row(
                &mut self.output,
                Feedback::Info(format!("{name} was kicked: {message}")),
            );
        }
        if let Some(player) = viewed_player {
            match run.session.clients.iter().position(|c| c.player == player) {
                Some(i) => run.view = rbx_sim::Peer::Client(i),
                None => {
                    let message = kicked.last().map(|(_, m)| m.clone()).unwrap_or_default();
                    run.view = rbx_sim::Peer::Server;
                    self.viewport.update(cx, |viewport, _| {
                        viewport.play = None;
                        viewport.core_gui.state = None;
                        viewport.core_gui.kicked = Some(message);
                    });
                    cx.notify();
                    return;
                }
            }
        }
        // Chat each client showed (only the viewed one's is drawn), and
        // what the player asked of the core GUI.
        let mut chat = Vec::new();
        for i in 0..run.session.clients.len() {
            if let Some(sim) = run.session.sim_mut(rbx_sim::Peer::Client(i)) {
                let lines = sim.take_chat();
                if run.view == rbx_sim::Peer::Client(i) {
                    chat = lines;
                }
            }
        }
        // `Emit`/`Clear` the viewed peer made, and the server's, which
        // every client sees.
        for peer in std::iter::once(rbx_sim::Peer::Server)
            .chain((0..run.session.clients.len()).map(rbx_sim::Peer::Client))
        {
            let shown = peer == run.view || peer == rbx_sim::Peer::Server;
            if let Some(sim) = run.session.sim_mut(peer) {
                for (emitter, burst) in sim.take_bursts() {
                    if shown {
                        rbx_viewer::particle_burst(
                            emitter,
                            match burst {
                                rbx_sim::Burst::Emit(n) => Some(n),
                                rbx_sim::Burst::Clear => None,
                            },
                        );
                    }
                }
            }
        }
        let (actions, menu_open) = self.viewport.update(cx, |viewport, _| {
            viewport.core_gui.push_chat(chat);
            (viewport.core_gui.take_actions(), viewport.core_gui.menu_open())
        });
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if let Some(sim) = run.session.sim_mut(run.view) {
            sim.set_menu_open(menu_open);
        }
        for action in actions {
            match action {
                CoreAction::Say(text) => {
                    if let Some(sim) = run.session.sim_mut(run.view) {
                        sim.chat_say(&text);
                    }
                }
                CoreAction::Reset => {
                    if let Some(sim) = run.session.sim_mut(run.view) {
                        sim.reset_character();
                    }
                }
                CoreAction::Volume(volume) => run.sound.set_volume(volume),
                CoreAction::Leave => {
                    self.local_run_stop(cx);
                    return;
                }
            }
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let changes = dedupe(changes);
        let Some(viewed) = run.session.sim(run.view) else {
            return;
        };
        run.sound.update(
            viewed.voices(),
            run.playing && !run.paused && viewing_client,
        );
        mirror(&viewed.dom(), &mut self.dom, &changes);
        let output = run.session.take_output();
        self.dom.take_changes();
        push_rows(&mut self.output, output, run.playing);
        if run.playing && viewing_client {
            let Some(viewed) = run.session.sim(run.view) else {
                return;
            };
            let mouse = viewed.mouse_behavior();
            let camera = viewed.camera();
            let player_gui = viewed.local_player().and_then(|(player, _)| {
                let dom = viewed.dom();
                dom.get(player)?
                    .children()
                    .iter()
                    .copied()
                    .find(|c| dom.get(*c).is_some_and(|i| i.class() == "PlayerGui"))
            });
            let core = run
                .session
                .sim_mut(run.view)
                .map(rbx_sim::Simulation::core_gui);
            self.show_player_gui(player_gui, cx);
            self.viewport.update(cx, |viewport, _| {
                if let Some(core) = core {
                    viewport.core_gui.update(core);
                }
                if let Some(play) = viewport.play.as_mut() {
                    play.mouse = mouse;
                }
                if let Some((cframe, fov)) = camera {
                    let (eye, target) = eye_and_target(&cframe);
                    viewport.look_at_with(eye, target, fov);
                }
            });
        }
        if !changes.is_empty() {
            let snapshots = self.dom.snapshot(&changes);
            let sent = changes.clone();
            self.viewport
                .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
            self.properties.dom_changed(&changes);
            if structural(&changes) {
                self.rebuild_explorer(cx);
            }
        }
        cx.notify();
    }

    /// Edits made to the shown copy during the run: written into the
    /// simulation's DOM (logged, so it routes them) and shown right away.
    pub(super) fn local_run_reflect(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        if let Some(run) = self.play.live.local.running.as_ref() {
            if let Some(sim) = run.session.sim(run.view) {
                let sim_dom = sim.dom_rc();
                mirror(&self.dom, &mut sim_dom.borrow_mut(), changes);
            }
        }

        let snapshots = self.dom.snapshot(changes);
        let sent = changes.to_vec();
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
        self.properties.dom_changed(changes);
        cx.notify();
    }
}

/// The game camera's eye and a point straight ahead of it (its -Z).
fn eye_and_target(camera: &rbx_dom::CFrameData) -> ([f32; 3], [f32; 3]) {
    let p = camera.position;
    let r = camera.rotation;
    let eye = [p.x, p.y, p.z];
    (eye, [p.x - r[2], p.y - r[5], p.z - r[8]])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

    // Not `super::*`: gpui_kit's glob brings its own `test` attribute.
    use super::{changes_between, dedupe, mirror, restore_edited, stash_edited};

    fn cframe(y: f32) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x: 0.0, y, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })
    }

    fn place_with_part() -> (WeakDom, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        let _ = dom.set_property(part, "CFrame", cframe(50.0));
        let _ = dom.set_property(
            part,
            "Size",
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        let _ = dom.set_property(part, "Anchored", Variant::Bool(false));
        dom.take_changes();
        (dom, workspace, part)
    }

    /// The tree as text, in tree order with sorted properties.
    fn fingerprint(dom: &WeakDom) -> String {
        let mut out = String::new();
        let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().rev().copied());
            let properties: BTreeMap<String, String> = instance
                .properties()
                .iter()
                .map(|(key, value)| (key.to_string(), format!("{value:?}")))
                .collect();
            out.push_str(&format!(
                "{} {} {} {:?}\n",
                at.value(),
                instance.class(),
                instance.name(),
                properties
            ));
        }
        out
    }

    #[test]
    fn local_run_mirrors_a_property_change() {
        let (mut sim, _, part) = place_with_part();
        let mut shown = sim.clone();
        let _ = sim.set_property(part, "CFrame", cframe(7.0));
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        assert_eq!(
            shown.get(part).and_then(|i| i.properties().get("CFrame")),
            Some(&cframe(7.0))
        );
    }

    #[test]
    fn local_run_mirrors_an_added_part() {
        let (mut sim, workspace, _) = place_with_part();
        let mut shown = sim.clone();
        let added = sim.new_instance("Part", "Spawned", Some(workspace));
        let _ = sim.set_property(added, "CFrame", cframe(3.0));
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        let copy = shown.get(added).expect("the part is shown");
        assert_eq!(copy.name(), "Spawned");
        assert_eq!(shown.parent(added), Some(workspace));
        assert_eq!(copy.properties().get("CFrame"), Some(&cframe(3.0)));
    }

    #[test]
    fn local_run_mirrors_a_removed_part() {
        let (mut sim, workspace, part) = place_with_part();
        let mut shown = sim.clone();
        sim.remove(part);
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        assert!(shown.get(part).is_none());
        assert!(shown
            .get(workspace)
            .is_some_and(|w| !w.children().contains(&part)));
    }

    #[test]
    fn local_run_restores_the_place_exactly() {
        let (place, _, part) = place_with_part();
        let before = fingerprint(&place);
        let mut dom = place;
        let mut sim = rbx_sim::Simulation::new(dom.clone()).expect("the simulation starts");
        sim.watch_view_changes();
        sim.start();
        let mut shown = sim.dom().clone();
        shown.take_changes();
        let edited = stash_edited(&mut dom, shown);
        for _ in 0..10 {
            sim.step();
            let changes = dedupe(sim.take_view_changes());
            mirror(&sim.dom(), &mut dom, &changes);
        }
        assert_ne!(
            dom.get(part).and_then(|i| i.properties().get("CFrame")),
            Some(&cframe(50.0)),
            "the part fell while running"
        );
        // The changes back, mirrored into the run's copy, give the place.
        let mut shown = dom.clone();
        let back = changes_between(&shown, &edited);
        assert!(!back.is_empty());
        mirror(&edited, &mut shown, &back);
        assert_eq!(fingerprint(&shown), before);
        restore_edited(&mut dom, edited);
        assert_eq!(fingerprint(&dom), before);
        assert!(dom.changes().is_empty());
    }
}
