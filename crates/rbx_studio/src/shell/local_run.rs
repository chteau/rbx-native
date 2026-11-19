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
use crate::command_bar::Feedback;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;

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
    sim: rbx_sim::Simulation,
    stash: Stash,
    paused: bool,
    /// Wall seconds the run has been unpaused for.
    clock: f64,
    last: Instant,
    /// Dropping it stops the frame loop.
    task: Option<Task<()>>,
    /// Play (local): a local player whose character the keys drive.
    playing: bool,
}

#[derive(Default)]
pub(crate) struct LocalRun {
    running: Option<Running>,
    /// Play (local) is fetching the avatar.
    pub(in crate::shell) starting: bool,
}

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

/// What the run's scripts printed, as game rows like real Play's.
fn push_rows(log: &mut super::output::OutputLog, output: &rbx_lua::Output) {
    for row in rows_of(output) {
        push_row(log, row);
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
        let mut sim = match rbx_sim::Simulation::new(self.dom.clone()) {
            Ok(sim) => sim,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Local Run couldn\u{2019}t start: {why}")),
                );
                cx.notify();
                return;
            }
        };
        sim.watch_view_changes();
        sim.set_fetcher(Arc::new(Fetcher(rbx_cloud::Client::new(
            rbx_cloud::ApiKey::from_env_or_config(),
        ))));
        let playing = player.is_some();
        let mut note = None;
        if let Some((player, avatars)) = player {
            sim.set_avatars(Box::new(avatars));
            if let Some(character) = player.character {
                sim.set_character(character);
            }
            sim.join(&player.name, player.user_id);
            note = Some(player.note);
        }
        sim.start();
        let mut shown = sim.dom().clone();
        shown.take_changes();
        let output = sim.take_output();
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
            sim,
            stash: Stash {
                dom,
                history,
                selection,
            },
            paused: false,
            clock: 0.0,
            last: Instant::now(),
            task: Some(task),
            playing,
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
        push_rows(&mut self.output, &output);
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

    pub(crate) fn local_run_stop(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.take() else {
            return;
        };
        let Running {
            mut sim,
            stash,
            task,
            ..
        } = run;
        drop(task);
        self.viewport
            .update(cx, |viewport, _| viewport.set_play_local(false));
        let output = sim.take_output();
        let seconds = sim.time();
        drop(sim);
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
        push_rows(&mut self.output, &output);
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
            return;
        }
        run.clock += elapsed;
        let due = ((run.clock * FRAME_HZ).floor() as u64).saturating_sub(run.sim.frame());
        if due == 0 {
            return;
        }
        let steps = due.min(MAX_FRAMES_PER_TICK);
        let input = self.viewport.read(cx).play.filter(|_| run.playing);
        let mut changes = Vec::new();
        for _ in 0..steps {
            if let Some(input) = &input {
                run.sim.control(input.direction(), input.jump);
            }
            run.sim.step();
            changes.extend(run.sim.take_view_changes());
        }
        if due > steps {
            run.clock = run.sim.time();
        }
        let changes = dedupe(changes);
        mirror(&run.sim.dom(), &mut self.dom, &changes);
        let output = run.sim.take_output();
        let focus = input.and_then(|input| Some((input, character_focus(&run.sim)?)));
        self.dom.take_changes();
        push_rows(&mut self.output, &output);
        if let Some((input, focus)) = focus {
            let eye = input.eye(focus);
            self.viewport
                .update(cx, |viewport, _| viewport.look_at(eye, focus));
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
            let sim_dom = run.sim.dom_rc();
            mirror(&self.dom, &mut sim_dom.borrow_mut(), changes);
        }
        let snapshots = self.dom.snapshot(changes);
        let sent = changes.to_vec();
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
        self.properties.dom_changed(changes);
        cx.notify();
    }
}

/// What the character camera looks at: a little above the local
/// character's root.
fn character_focus(sim: &rbx_sim::Simulation) -> Option<[f32; 3]> {
    let (_, character) = sim.local_player()?;
    let dom = sim.dom();
    let model = dom.get(character?)?;
    let root = model
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == "HumanoidRootPart"))?;
    match dom.get(root)?.properties().get("CFrame") {
        Some(rbx_dom::Variant::CFrame(c)) => Some([
            c.position.x,
            c.position.y + crate::workspace_view::FOCUS_ABOVE_ROOT,
            c.position.z,
        ]),
        _ => None,
    }
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
