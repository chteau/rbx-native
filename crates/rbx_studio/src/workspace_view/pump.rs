//! Rendering off the UI thread.
//!
//! One thread owns the viewer and does the whole GPU round trip — advance the
//! camera, draw, read back — and hands finished frames over a channel. The UI
//! thread does nothing but upload them, so neither the 5 MB readback nor the wait
//! for the GPU behind it ever sits between a mouse move and the answer to it.
//!
//! The thread paces itself to one display refresh and draws continuously while
//! the panel is visible — camera at rest or not — so animated content
//! (particles, `Trail`, `Clouds`) keeps moving without input; it sleeps
//! outright once the panel is not, which is what keeps a backgrounded view
//! free.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rbx_dom::{Change, Ref, Snapshot, WeakDom};
use rbx_viewer::pick::{Meshes, Selected};
use rbx_viewer::{Applied, CameraInput, Gizmo, Headless, Pose, QualityLevel};

use super::quality::Quality;
use super::stats::Stats;

/// How long the thread sleeps between checks while the panel is not visible.
/// Short enough that closing the window is not noticeably slower, long enough
/// that a backgrounded view costs nothing.
const IDLE_WAIT: Duration = Duration::from_millis(100);
/// The longest step the camera may be advanced by, in frame budgets. A thread
/// that just woke from a long idle would otherwise fly the camera across the map
/// in the single frame that follows the key which woke it.
const MAX_STEPS: u32 = 3;
/// How often a moved pose is worth reporting up to the UI thread for DOM sync
/// (see `WorkspaceView`'s `PoseSynced` event) — frames themselves come at
/// ~75/sec (see this module's doc comment), far more often than
/// `Workspace.CurrentCamera.CFrame` needs to track a flying camera.
const POSE_SYNC_INTERVAL: Duration = Duration::from_millis(200);

enum Command {
    Input(CameraInput),
    Size((u32, u32)),
    Quality(QualityLevel),
    Orthographic(bool),
    Selection(Vec<Selected>),
    /// The "about to click" cue — see `Headless::set_hover`. `None` clears
    /// it, the way an empty `Selection` clears the selection outline.
    Hover(Option<Ref>),
    /// Which transform tool's draggers to draw over the selection, if any —
    /// see `Headless::set_gizmo`.
    Gizmo(Option<Gizmo>),
    /// An edit to the DOM, as the `Change` log it produced, patched into the
    /// scene instance by instance — see `Headless::apply_changes`. What
    /// travels with the log is a snapshot of the instances it names (see
    /// `WeakDom::snapshot`), never the tree: the thread keeps a mirror of
    /// the editor's DOM and brings it in step first, so one command costs
    /// the edit, not the place. The few edits that still need a full
    /// rebuild (see `rbx_viewer::Rebuild`) get one right there on the render
    /// thread, and are reported on stderr so the reason is on record.
    Changes(Vec<Snapshot>, Vec<Change>),
    Visible(bool),
    Stop,
}

/// A frame to upload, carrying the flight speed and the graphics quality level as
/// of the moment it was drawn. `pixels` is empty on a message sent only to report
/// one of those two changing. `pose` is `Some` only on the (throttled) tick that
/// is due to report it — see [`POSE_SYNC_INTERVAL`] and [`due_pose`] — so most
/// messages carry `None` even while the camera is moving every frame.
pub(super) struct Ready {
    pub(super) pixels: Option<Vec<u8>>,
    pub(super) size: (u32, u32),
    pub(super) speed: f32,
    pub(super) level: u8,
    pub(super) pose: Option<Pose>,
    /// The camera this frame was actually drawn from, on every message rather
    /// than on the throttled tick `pose` uses. What the viewport unprojects a
    /// click with: a pick resolved against a pose up to `POSE_SYNC_INTERVAL`
    /// stale would select whatever was under the cursor a fifth of a second
    /// ago, which is exactly the moment after flying the camera when a user is
    /// most likely to click.
    pub(super) view: Option<Pose>,
    /// The file meshes the viewer's scene holds, on the first message and on
    /// the one after every rebuild — see `Headless::pick_meshes`. `None` the
    /// rest of the time: the geometry a click is tested against only changes
    /// when the scene does, and a handle onto it is all the UI thread keeps.
    pub(super) meshes: Option<Meshes>,
    /// Asset-fetch/decode warnings drained off the viewer since the previous
    /// tick — see `Headless::drain_warnings`. Empty on most ticks, same as
    /// `pose`, but unlike `pose` this is never throttled: a warning is worth
    /// showing the moment it exists, not on a sampled interval.
    pub(super) warnings: Vec<String>,
}

pub(super) struct Pump {
    commands: Sender<Command>,
    ready: Receiver<Ready>,
    stats: Arc<Stats>,
    /// Joined on drop: the thread holds a GPU device of its own, and letting it
    /// draw into a window that is being torn down is how a driver crash starts.
    thread: Option<JoinHandle<()>>,
}

impl Pump {
    /// `dom` is the tree `viewer` was built from, handed over once: the
    /// thread keeps it as its mirror of the editor's DOM from here on — see
    /// [`Command::Changes`].
    pub(super) fn spawn(
        viewer: Headless,
        dom: WeakDom,
        interval: Duration,
        quality: QualityLevel,
    ) -> Self {
        let (commands, orders) = mpsc::channel();
        let (frames, ready) = mpsc::channel();
        let stats = Arc::new(Stats::default());
        let counters = stats.clone();

        let thread = thread::Builder::new()
            .name("rbxstudio-render".to_string())
            .spawn(move || {
                run(
                    viewer,
                    dom,
                    &orders,
                    &frames,
                    &counters,
                    Opened { interval, quality },
                )
            })
            .ok();
        if thread.is_none() {
            eprintln!("rbxstudio: no render thread could be started");
        }

        Pump {
            commands,
            ready,
            stats,
            thread,
        }
    }

    pub(super) fn input(&self, event: CameraInput) {
        let _ = self.commands.send(Command::Input(event));
    }

    pub(super) fn resize(&self, size: (u32, u32)) {
        let _ = self.commands.send(Command::Size(size));
    }

    /// Switches the graphics quality mode, `Automatic` included.
    pub(super) fn quality(&self, mode: QualityLevel) {
        let _ = self.commands.send(Command::Quality(mode));
    }

    /// Swaps the main camera between perspective and orthographic projection.
    pub(super) fn orthographic(&self, orthographic: bool) {
        let _ = self.commands.send(Command::Orthographic(orthographic));
    }

    /// Outlines what `selected` covers in the viewport.
    pub(super) fn select(&self, selected: Vec<Selected>) {
        let _ = self.commands.send(Command::Selection(selected));
    }

    /// Outlines the hovered part in the viewport, distinctly from the
    /// selection — `None` clears it.
    pub(super) fn hover(&self, referent: Option<Ref>) {
        let _ = self.commands.send(Command::Hover(referent));
    }

    /// Shows or hides the transform tool's draggers over the selection.
    pub(super) fn gizmo(&self, gizmo: Option<Gizmo>) {
        let _ = self.commands.send(Command::Gizmo(gizmo));
    }

    /// Patches the viewer's scene for one edit's `Change` log — see
    /// [`Command::Changes`].
    pub(super) fn apply_changes(&self, snapshots: Vec<Snapshot>, changes: Vec<Change>) {
        let _ = self.commands.send(Command::Changes(snapshots, changes));
    }

    /// Tells the render thread whether the panel is actually on screen — the
    /// dock only mounts the active tab of a group, so a hidden viewport never
    /// calls back into `render` to report a size. Draws pace continuously
    /// while visible (so animated content — particles, `Trail`, `Clouds` —
    /// keeps moving without camera input) and stop entirely while not, rather
    /// than drawing forever into a picture nobody can see.
    pub(super) fn set_visible(&self, visible: bool) {
        let _ = self.commands.send(Command::Visible(visible));
    }

    /// The next message waiting, without ever blocking the UI thread.
    pub(super) fn poll(&self) -> Option<Ready> {
        self.ready.try_recv().ok()
    }

    pub(super) fn stats(&self) -> &Stats {
        &self.stats
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What the thread opens with: the frame budget it paces itself to, and the
/// quality mode the command line asked for.
struct Opened {
    interval: Duration,
    quality: QualityLevel,
}

fn run(
    mut viewer: Headless,
    mut mirror: WeakDom,
    commands: &Receiver<Command>,
    frames: &Sender<Ready>,
    stats: &Stats,
    opened: Opened,
) {
    let Opened { interval, quality } = opened;
    let mut quality = Quality::new(quality, interval, &mut viewer);
    let mut size = (0, 0);
    let mut visible = true;
    let mut was_visible = false;
    let mut ticked = Instant::now();
    let mut speed = viewer.speed();
    let mut idle = false;
    let mut due = Instant::now();
    let mut shown = viewer.quality().resolved();
    // `Instant::now()`, not `ticked`: due right away, so a place that opens
    // already aimed at its own saved `Camera` reports that pose once even with
    // zero input — see `due_pose`'s doc comment.
    let mut pose_due = Instant::now();
    let mut pose_sent = None;
    // True to begin with: the scene the thread opened with is as new to the
    // UI thread as any rebuilt one.
    let mut rebuilt = true;

    loop {
        if !drain(
            commands,
            Rendering {
                viewer: &mut viewer,
                mirror: &mut mirror,
                quality: &mut quality,
                size: &mut size,
                visible: &mut visible,
                rebuilt: &mut rebuilt,
            },
            idle,
        ) {
            return;
        }

        let now = Instant::now();
        viewer.tick(step(now.duration_since(ticked), interval));
        ticked = now;

        let frame = next_frame(&mut viewer, size, visible, &mut was_visible, stats);
        // Not visible, or a frame just drained on the way to that: wait for an
        // event rather than for the clock. While visible, a frame is always
        // owed — that is what keeps animated content moving without input.
        idle = !visible && frame.is_none();

        let current = viewer.speed();
        let told = (current - speed).abs() > f32::EPSILON;
        speed = current;
        let cost = frame.as_ref().map(|frame| frame.cost);
        if let Some(frame) = &frame {
            shown = frame.level;
        }

        let view = viewer.pose();
        let pose = due_pose(view, pose_sent, now >= pose_due);
        if let Some(pose) = pose {
            pose_sent = Some(pose);
            pose_due = now + POSE_SYNC_INTERVAL;
        }

        let warnings = viewer.drain_warnings();
        let meshes = std::mem::take(&mut rebuilt).then(|| viewer.pick_meshes());

        if (frame.is_some() || told || pose.is_some() || !warnings.is_empty() || meshes.is_some())
            && frames
                .send(Ready {
                    pixels: frame.map(|frame| frame.pixels),
                    size,
                    speed,
                    level: shown,
                    pose,
                    view,
                    meshes,
                    warnings,
                })
                .is_err()
        {
            return;
        }

        // After the frame was handed over, never before: a level change rebuilds
        // the renderer, and the upload it would otherwise wait behind is the
        // whole reason this thread exists.
        if let Some(cost) = cost {
            quality.record(cost + stats.last_upload(), &mut viewer);
        }

        stats.report(interval, shown);
        let now = Instant::now();
        due = deadline(due, interval, now);
        if idle {
            // Nothing to be on time for: the next frame starts when the next
            // event does.
            due = now;
        } else if let Some(rest) = due.checked_duration_since(now) {
            thread::sleep(rest);
        }
    }
}

/// When the next frame is due: one budget after the last one was, never in the
/// past.
///
/// Sleeping "what is left of the budget" instead would lose a frame every few:
/// each sleep overshoots by a millisecond or two, and counted from `now` that
/// overshoot is never made up — 75 Hz asked for, 60 delivered.
fn deadline(previous: Instant, interval: Duration, now: Instant) -> Instant {
    (previous + interval).max(now)
}

/// A frame off the GPU, with what it cost this thread and the quality level it
/// was drawn at.
struct Frame {
    pixels: Vec<u8>,
    /// Render plus readback. The upload is the UI thread's share and is added
    /// where the manager is fed, not here.
    cost: Duration,
    level: u8,
}

/// Draws the frame the viewport is owed, or collects the one still in flight.
///
/// Panels the dock is not currently showing never report a size — the active
/// tab of a group is the only one mounted, so a hidden viewport's `render`
/// simply never runs to say otherwise — but `visible` is driven by that same
/// absence (see `WorkspaceView::advance`), which is what actually stops the
/// draw: a size left over from before the tab was switched away is not enough
/// on its own. Going invisible still has to collect the frame queued just
/// before it happened, or the view would stand one frame short once it comes
/// back; going visible again starts drawing every tick, at rest or not, which
/// is what keeps animated content (particles, `Trail`, `Clouds`) moving
/// without camera input.
fn next_frame(
    viewer: &mut Headless,
    size: (u32, u32),
    visible: bool,
    was_visible: &mut bool,
    stats: &Stats,
) -> Option<Frame> {
    if size.0 == 0 || size.1 == 0 || !visible {
        let draining = std::mem::replace(was_visible, false);
        return if draining {
            drain_pending_frame(viewer, stats)
        } else {
            None
        };
    }
    *was_visible = true;

    // Read before the frame is queued, because that is the frame it applies to:
    // what comes back below was queued at the previous call, so a level that
    // just changed reaches the label one frame early.
    let level = viewer.quality().resolved();
    let rendered = viewer.render_frame(size.0, size.1);

    match rendered {
        Ok(Some(rendered)) => {
            stats.drew(rendered.render, rendered.readback);
            Some(Frame {
                pixels: rendered.pixels,
                cost: rendered.render + rendered.readback,
                level,
            })
        }
        Ok(None) => None,
        Err(err) => {
            eprintln!("rbxstudio: {err}");
            None
        }
    }
}

/// Collects the one frame `render_frame` had already queued when the viewport
/// stopped being visible, without queuing another behind it.
fn drain_pending_frame(viewer: &mut Headless, stats: &Stats) -> Option<Frame> {
    let level = viewer.quality().resolved();
    match viewer.take_frame() {
        Ok(Some(rendered)) => {
            stats.drew(rendered.render, rendered.readback);
            Some(Frame {
                pixels: rendered.pixels,
                cost: rendered.render + rendered.readback,
                level,
            })
        }
        Ok(None) => None,
        Err(err) => {
            eprintln!("rbxstudio: {err}");
            None
        }
    }
}

/// Everything a command may reach: the viewer, what it draws at, the size it
/// draws at and whether the panel is currently visible.
struct Rendering<'a> {
    viewer: &'a mut Headless,
    /// The editor's DOM as this thread last saw it — see [`Command::Changes`].
    mirror: &'a mut WeakDom,
    quality: &'a mut Quality,
    size: &'a mut (u32, u32),
    visible: &'a mut bool,
    /// Set by any command that rebuilt the scene, so the tick that follows
    /// reports the geometry the UI thread now has to pick against — see
    /// [`Ready::meshes`].
    rebuilt: &'a mut bool,
}

/// Applies everything the UI thread has asked for, blocking for the first order
/// only while the camera is at rest. Returns `false` once the view is gone.
fn drain(commands: &Receiver<Command>, mut rendering: Rendering<'_>, idle: bool) -> bool {
    if idle {
        match commands.recv_timeout(IDLE_WAIT) {
            Ok(command) => {
                if !apply(command, &mut rendering) {
                    return false;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }

    loop {
        match commands.try_recv() {
            Ok(command) => {
                if !apply(command, &mut rendering) {
                    return false;
                }
            }
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
}

fn apply(command: Command, rendering: &mut Rendering<'_>) -> bool {
    match command {
        Command::Input(event) => rendering.viewer.input(event),
        Command::Size(new) => *rendering.size = new,
        Command::Quality(mode) => rendering.quality.set(mode, rendering.viewer),
        Command::Orthographic(orthographic) => rendering.viewer.set_orthographic(orthographic),
        Command::Selection(selected) => rendering.viewer.set_selection(&selected),
        Command::Hover(referent) => rendering.viewer.set_hover(referent),
        Command::Gizmo(gizmo) => rendering.viewer.set_gizmo(gizmo),
        Command::Changes(snapshots, changes) => {
            rendering.mirror.mirror(snapshots);
            match rendering.viewer.apply_changes(rendering.mirror, &changes) {
                Ok(Applied::Patched) => {}
                Ok(Applied::Rebuilt(why)) => {
                    *rendering.rebuilt = true;
                    eprintln!("rbxstudio: scene rebuilt: {why}");
                }
                Err(err) => eprintln!("rbxstudio: edit failed: {err}"),
            }
        }
        Command::Visible(new) => *rendering.visible = new,
        Command::Stop => return false,
    }

    true
}

/// How far to advance the camera for a frame that took `since` to come round.
fn step(since: Duration, interval: Duration) -> Duration {
    since.min(interval * MAX_STEPS)
}

/// Whether this tick's pose (`candidate`, `None` while the view is still
/// auto-orbiting — see [`Headless::pose`]) is worth reporting up to the UI
/// thread: gated on [`POSE_SYNC_INTERVAL`] (`ready`) and on actually differing
/// from the last one sent, so a camera at rest — the common case, since a
/// frame is drawn every tick regardless of movement — never re-reports the
/// same value once `ready` comes back around.
///
/// A plain function of three values rather than a method on any of the loop's
/// running state, so it can be unit-tested the way [`step`] and [`deadline`]
/// are, with no `Headless` or GPU device involved.
fn due_pose<T: Copy + PartialEq>(
    candidate: Option<T>,
    last_sent: Option<T>,
    ready: bool,
) -> Option<T> {
    let pose = candidate?;
    if !ready || last_sent == Some(pose) {
        return None;
    }
    Some(pose)
}

#[cfg(test)]
#[path = "pump/tests.rs"]
mod tests;
