//! The 3D view: the wgpu viewer rendered offscreen and shown as a plain image.
//! It opens on the viewpoint the place was saved at, the way Studio does, and
//! flies from there with the same free-flight camera the standalone viewer's
//! window has (right button to look, WASD positions to move, wheel to step or —
//! with the button held — to change speed).
//!
//! GPUI draws on its own wgpu device, which cannot be shared with the viewer's,
//! so every frame travels through system memory: render, read back, upload. All
//! of that but the upload happens on a thread of its own (see [`pump`]); this
//! module is the UI half — events in, finished frames out.

mod changes;
mod frame;
mod gizmo;
mod hover;
mod input;
mod label;
mod presence;
mod pump;
mod quality;
mod scroll;
mod stats;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat3, Mat4, Vec3};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_viewer::pick::{Meshes, Ray, Selected};
use rbx_viewer::{CameraInput, Headless, Pose, QualityLevel};

use crate::camera::PlaceCamera;
use crate::pointer_lock::{self, PointerLock};
use crate::settle::Settle;
use crate::transform::{self, Targets, Transform};
use crate::{display, pacing};
use frame::{device_pixels, render_image, Viewport};
use gizmo::Drag;
use input::{camera_key, chorded, tool_key, Layout};
use pump::Pump;
pub(crate) use scroll::{scrolled, Scroll};

// How long a speed change stays on screen, matching the standalone viewer's
// title bar.
const SPEED_LABEL: Duration = Duration::from_millis(1500);
// How many times per frame budget the UI thread looks for a finished frame. Two
// loops running at the same rate beat against each other, and every beat is a
// frame shown a whole budget late; waking twice as often costs one `try_recv`.
const POLLS_PER_FRAME: u32 = 2;

/// The render thread's latest free-flight pose, throttled and deduplicated
/// already (see `pump::due_pose`) — emitted at most a few times a second while
/// the camera is actually moving, never on every drawn frame. `Shell`
/// subscribes to mirror it into `Workspace.CurrentCamera.CFrame` (see
/// `crate::camera::write_pose`), through a write that deliberately never goes
/// near `History::push`: nothing about flying the camera is meant to be
/// undoable.
pub(crate) struct PoseSynced(pub(crate) Pose);

impl EventEmitter<PoseSynced> for WorkspaceView {}

/// Asset-fetch/decode warnings drained off the render thread this tick,
/// oldest first — see `pump::Ready::warnings`. `Shell` subscribes to append
/// each one to the Output dock (`shell::output::OutputLog::push_warning`),
/// the same way it mirrors `PoseSynced` into the camera.
pub(crate) struct AssetWarnings(pub(crate) Vec<String>);

impl EventEmitter<AssetWarnings> for WorkspaceView {}

/// Something the 3D view asks of the editor.
///
/// The view owns the cursor and knows the camera; `Shell` owns the DOM and the
/// undo stack. Everything the left mouse button does in the viewport therefore
/// arrives here rather than being carried out on the spot.
pub(crate) enum ViewportAction {
    /// A click, as the world ray under it — `Shell` resolves what that ray
    /// actually hits (see `shell::selection::from_click`). `extend` is
    /// `Shift`/`Ctrl`/`Cmd` held: add the hit to the selection (or drop it, if
    /// it was already in) rather than replacing the selection with it.
    ///
    /// `held`: the view has a Move body-drag ready for this same click, to
    /// start only if what is actually under the cursor is already selected
    /// (see [`WorkspaceView::confirm_grab`]) — the view sees the selection's
    /// boxes, not whatever unselected part may stand in front of them, and a
    /// click on that part must select it, not drag the selection behind it.
    Pick {
        ray: Ray,
        cycling: bool,
        extend: bool,
        held: bool,
    },
    /// A drag moved every part it carries to a new position — more than one
    /// when the gesture grabbed a multi-part selection's gizmo, each keeping
    /// its offset from the others (see `transform::Targets::translate`).
    /// `first` marks the move that began the gesture, which is the single
    /// undo step the whole drag gets: pushing one per mouse move would bury
    /// the rest of the history in a fraction of a second.
    ///
    /// A cursor drag on a single part also carries a [`Settle`], asking
    /// `Shell` to rest it on whatever the cursor is over instead — the move
    /// already computed is then only the fallback for a cursor over nothing.
    /// A group drag never settles: cursor dragging is Move's own one-part
    /// body-grab gesture (see `gizmo::Drag::Plane`), and a multi-part
    /// selection's gizmo only ever grabs a handle.
    Moved {
        moves: Vec<(Ref, Vec3)>,
        first: bool,
        settle: Option<Settle>,
    },
    /// A Scale drag resized the part. The centre travels with it: the face
    /// opposite the grabbed one holds still, so growing the part by a stud
    /// moves its middle by half of one.
    Resized {
        /// Each part's new size and centre — one entry for a lone part's
        /// own Scale, every selected part for a group scaled as a whole (see
        /// `transform::Targets::scale_about`).
        parts: Vec<(Ref, Vec3, Vec3)>,
        first: bool,
    },
    /// A Rotate drag turned the part about its centre, which is where the
    /// rings stand. Only the `CFrame`'s rotation changes.
    Rotated {
        /// Each part's new orientation and centre — the centre unchanged for
        /// a lone part turning about itself, swung round the selection's
        /// centre for every part of a group (see
        /// `transform::Targets::rotate_about`).
        parts: Vec<(Ref, Mat3, Vec3)>,
        first: bool,
    },
    /// `T` or `R` during a cursor drag: a quarter turn about `pivot`, the
    /// point the part is being held by. `first` marks the gesture's undo step,
    /// exactly as `Moved` does — a drag that turns the part and then moves it
    /// is still one drag.
    Turned {
        referent: Ref,
        pivot: Vec3,
        axis: Vec3,
        first: bool,
    },
    /// A transform-toolbar shortcut typed over the view.
    Tool(transform::Action),
    /// Cursor motion with nothing held: `Shell` resolves what is under the
    /// ray and outlines it, distinctly from the selection outline — Studio's
    /// "about to click" cue (see `rbx_viewer::renderer::hover`). `alt` is the
    /// selection-cycling modifier held, so the outline previews what a click
    /// would land on: the whole enclosing `Model` plain, the single part
    /// under the cursor with `Alt`. `ray` `None` clears the outline outright
    /// rather than leaving it to resolve to nothing on its own: the cursor
    /// left the panel (see `render`'s `on_hover`), or a drag or camera look
    /// just began and a hover box hanging over the gesture would look
    /// broken.
    Hover { ray: Option<Ray>, alt: bool },
    /// The wheel rolled over a `ScrollingFrame` of a `ScreenGui`: `Shell`
    /// moves its `CanvasPosition` (see `shell::scroll`). Not an edit —
    /// Studio's own scroll is not undoable either — so it never goes near
    /// the undo stack, the way a camera pose does not.
    Scrolled(Scroll),
}

impl EventEmitter<ViewportAction> for WorkspaceView {}

/// What a view opens on: the viewer, and the tree it was built from, which
/// the render thread keeps as its own copy of the editor's DOM from then on
/// — see `pump::Command::Changes`.
pub(crate) struct Opened {
    pub(crate) viewer: Headless,
    pub(crate) dom: WeakDom,
}

pub(crate) struct WorkspaceView {
    /// The render thread. It owns the viewer, which is why no camera state is
    /// readable from here — only what it reports back with each frame.
    pump: Pump,
    /// Keys only reach the camera while the viewport holds focus, so clicking it
    /// is what takes focus away from the Explorer's search boxes.
    focus: FocusHandle,
    /// Where the cursor sat at the previous move, for the unlocked fallback
    /// path: GPUI reports positions, and the camera wants deltas. Cleared when
    /// the look button lands, so the press itself never turns the view; a drag
    /// that wanders off the panel and back keeps the old reference on purpose,
    /// which turns the view by the whole travel rather than losing the part GPUI
    /// could not report. Unused while [`PointerLock`] holds the pointer, which
    /// measures the travel against the centre it warps back to instead.
    cursor: Option<Point<Pixels>>,
    lock: PointerLock,
    /// Whether a look gesture (right-button orbit) is in progress right now
    /// — independent of whether `PointerLock` actually captured the OS
    /// pointer, which it never does on Wayland (see `pointer_lock`'s module
    /// doc). What hover resolution gates on instead of `PointerLock::holds`
    /// (see `hover::suppressed`).
    looking: bool,
    /// The latest un-resolved cursor position from an ordinary move,
    /// waiting for `advance` to decide it's due (see `hover::due`) and cast
    /// a ray against the DOM. Cleared, not just left to resolve stale, at
    /// every place that already emits `ViewportAction::Hover(None)` — a drag
    /// starting (`gizmo::press`), a look starting (`begin_look`), and the
    /// cursor leaving the panel (`render`'s `on_hover`) — or a throttled
    /// move would otherwise un-clear it a moment later.
    hover_pending: Option<(Point<Pixels>, Modifiers)>,
    /// The cursor's latest position and modifiers while a drag is held, not
    /// yet applied to the part — applied by `advance` at most once per
    /// frame, and by the release (see `gizmo::WorkspaceView::end_drag`).
    drag_pending: Option<(Point<Pixels>, Modifiers)>,
    /// When the drag in progress last stepped, for that once-per-frame gate.
    drag_stepped_at: Option<Instant>,
    /// When the hover ray was last actually resolved, for `hover::due`.
    hover_resolved_at: Option<Instant>,
    /// The one wheel event `RBX_STUDIO_SCROLL` asks for (see [`scroll`]),
    /// held until the first frame is up: the render thread can only hit-test
    /// an overlay it has laid out, which it does on that first draw.
    debug_wheel: Option<([f32; 2], input::Wheel)>,
    /// The panel's place in the window, in physical pixels. Written during
    /// layout and read afterwards, which is why it is shared rather than passed:
    /// both happen on the UI thread, never at the same time.
    viewport: Rc<Cell<Viewport>>,
    /// The size the render thread was last told to draw at.
    sized: (u32, u32),
    /// Set by `render` each time GPUI actually paints this view, and cleared by
    /// `advance`, which runs on a plain timer of its own and keeps firing
    /// whether or not the dock currently mounts this panel. The dock renders
    /// only the active tab of a group (see `gpui_component::dock::tab_panel`),
    /// so a viewport switched away from stops calling `render` entirely — this
    /// is the only signal that tells `advance` so, since the panel's last known
    /// size otherwise just sits there unchanged forever.
    painted: Rc<Cell<bool>>,
    /// Whether the render thread was last told the panel is visible, and how
    /// many `advance` ticks in a row have found `painted` unset since. See
    /// [`presence`].
    visible: bool,
    missed_paints: u32,
    frame: Option<Arc<RenderImage>>,
    /// One display refresh: the budget a frame is given while the window is
    /// focused. Fixed for the life of the view — see `pacing` for the
    /// unfocused cap this and the window's focus state combine into.
    full_interval: Duration,
    /// What the render loop is currently paced to: `full_interval` while
    /// focused, or the capped unfocused rate otherwise — see [`pacing::FocusPacing`].
    /// Drives the UI thread's own poll delay (`advance`'s return value) and
    /// the hover/drag gates below; pushed to the render thread itself
    /// through `Pump::set_interval` whenever `pacing` says it changed.
    interval: Duration,
    /// Whether the window currently has OS focus, and the unfocused fps
    /// preset — see [`pacing::FocusPacing`].
    pacing: pacing::FocusPacing,
    speed: f32,
    speed_shown_until: Option<Instant>,
    /// The quality mode the user picked, and the level the render thread last
    /// reported drawing at. In `Automatic` the two differ by design: the mode is
    /// the choice, the level is what the frame rate allows.
    quality: QualityLevel,
    level: u8,
    /// The projection mode the user last picked from the Viewport panel's
    /// overflow menu (see `Shell::set_orthographic`).
    orthographic: bool,
    /// Whether the corner label shows `pump.stats()`'s frame rate — the
    /// Viewport panel overflow menu's Stats toggle, next to Orthographic
    /// (see `Shell::set_stats_shown`). Session-only: real Studio's own
    /// `Window > Performance > Stats` doesn't persist across restarts
    /// either.
    stats_shown: bool,
    /// The transform toolbar's state, pushed down from `Shell` (see
    /// [`WorkspaceView::set_transform`]).
    transform: Transform,
    /// Where every selected part stands, so a click can be hit-tested against
    /// the draggers here rather than on the render thread, and a drag can
    /// move the whole selection together (see `transform::Targets`).
    targets: Targets,
    /// The boxes every other drawn part occupies, for a free drag to soft-snap
    /// onto (see `gizmo::Landing`). Pushed down from `Shell` when the
    /// selection changes rather than read per move: only the dragged part is
    /// moving, so its neighbours stand still for the length of a gesture.
    neighbours: Vec<Mat4>,
    /// The camera the last frame was drawn from — see `pump::Ready::view`.
    /// Everything screen-to-world unprojects against this, so a click resolves
    /// against the view it was aimed at.
    view: Option<Pose>,
    /// The file meshes resident on the render thread, as of the last scene it
    /// built — see `pump::Ready::meshes`. What `Shell` picks a `MeshPart`
    /// against, so a click resolves against the triangles on screen rather
    /// than a box around them; a handle onto the renderer's own data, not a
    /// copy of it.
    meshes: Meshes,
    drag: Option<Drag>,
    /// A body grab the last press found on the selection, held back until
    /// `Shell` has resolved the same click against the real geometry (see
    /// `ViewportAction::Pick`'s `held`).
    pending_grab: Option<Drag>,
    /// The selection as it stood when the drag in progress grabbed it: what
    /// a group Scale or Rotate measures from, so a gesture is one absolute
    /// factor or turn rather than a running product (see
    /// `transform::Targets::scale_about`).
    held: Targets,
    /// Whether the drag in progress has actually moved the part yet, which is
    /// what tells `Shell` which move opens the gesture's one undo step.
    dragged: bool,
    /// Kept only to stay subscribed: dropping these unregisters the listeners.
    _subscriptions: [Subscription; 2],
}

impl WorkspaceView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        opened: Opened,
        camera: Option<PlaceCamera>,
        quality: QualityLevel,
        orthographic: bool,
        unfocused_fps: pacing::UnfocusedFps,
        selected: Vec<Selected>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Opened { mut viewer, dom } = opened;
        // A file with no camera of its own still has to be shown somehow: the
        // viewer then orbits its bounds until the first input, exactly as
        // `rbxview --orbit` does.
        if let Some(camera) = camera {
            viewer.open_at(camera.eye, camera.look_at, camera.fov_degrees);
        }
        viewer.set_selection(&selected);
        viewer.set_orthographic(orthographic);

        let full_interval = pacing::frame_interval(display::refresh_hz());
        let pacing = pacing::FocusPacing::new(unfocused_fps);
        // A freshly opened window is focused (see `FocusPacing::new`), so the
        // loop opens at the full rate; `pacing` only starts mattering once
        // something blurs or deactivates it.
        let interval = pacing.interval(full_interval);
        let speed = viewer.speed();
        cx.spawn_in(window, async move |view, cx| {
            let mut delay = interval;
            loop {
                cx.background_executor().timer(delay).await;
                // The window is gone once the update fails; nothing left to draw.
                let Ok(remaining) = view.update_in(cx, |view, window, cx| view.advance(window, cx))
                else {
                    return;
                };
                delay = remaining;
            }
        })
        .detach();

        let focus = cx.focus_handle();
        // Focused up front so the keyboard flies the camera without a click first.
        window.focus(&focus, cx);
        // Nothing held survives losing focus: the release events would be
        // delivered elsewhere, leaving the camera drifting for good and the
        // cursor hidden with no way to bring it back.
        let blur = cx.on_blur(&focus, window, |view, _, _| {
            view.end_look();
        });
        // Alt-tabbing away never blurs the focus handle, so the window's own
        // activation is watched too — and, since it's the authoritative OS
        // focus signal (unlike the in-app `focus` handle above), it's also
        // what drives the render-rate throttle (see `pacing`).
        let deactivated = cx.observe_window_activation(window, |view, window, _| {
            let active = window.is_window_active();
            if !active {
                view.end_look();
            }
            if view.pacing.set_active(active) {
                view.retarget_pacing();
            }
        });

        WorkspaceView {
            pump: Pump::spawn(viewer, dom, interval, quality),
            focus,
            cursor: None,
            lock: PointerLock::new(),
            looking: false,
            hover_pending: None,
            drag_pending: None,
            drag_stepped_at: None,
            hover_resolved_at: None,
            debug_wheel: scroll::debug_wheel(),
            viewport: Rc::new(Cell::new(Viewport::default())),
            sized: (0, 0),
            // Assumed visible until `advance` first has a chance to find out
            // otherwise: `render` runs once as part of mounting, ahead of the
            // first `advance` tick, in the ordinary case of an already-visible
            // panel.
            painted: Rc::new(Cell::new(true)),
            visible: true,
            missed_paints: 0,
            frame: None,
            full_interval,
            interval,
            pacing,
            speed,
            speed_shown_until: None,
            quality,
            level: QualityLevel::MAX,
            orthographic,
            stats_shown: false,
            transform: Transform::default(),
            targets: Targets::default(),
            neighbours: Vec::new(),
            view: None,
            meshes: Meshes::default(),
            drag: None,
            pending_grab: None,
            held: Targets::default(),
            dragged: false,
            _subscriptions: [blur, deactivated],
        }
    }

    /// The file meshes the render thread last reported drawing — see the
    /// field. `Shell` reads this when it resolves a click, since the DOM it
    /// picks against lives there and the geometry lives here.
    pub(crate) fn meshes(&self) -> &Meshes {
        &self.meshes
    }

    /// Puts whatever the render thread has finished on screen, resolves a
    /// pending hover ray if one is due (see `hover::due`), and returns how
    /// long the loop should wait before looking again.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Duration {
        let now = Instant::now();

        // `render` not having run since the last tick may mean the dock
        // switched away to another tab: `viewport`'s size would otherwise sit
        // stale at whatever it last was, so this is the only way to catch it.
        // A single miss is not enough on its own, and a run of them is not
        // conclusive either — see `presence`.
        let presence = presence::presence(
            self.painted.replace(false),
            self.missed_paints,
            self.visible,
        );
        self.missed_paints = presence.missed;
        if presence.visible != self.visible {
            self.visible = presence.visible;
            self.pump.set_visible(presence.visible);
        }
        if presence.probe {
            // Nothing else will repaint this panel while the render thread is
            // stalled, and its own silence is the only evidence that it might
            // be hidden — see `presence`.
            cx.notify();
        }

        let size = self.viewport.get().size;
        if self.sized != size {
            self.sized = size;
            self.pump.resize(size);
        }

        // Everything waiting is drained, not just the first: an overtaken frame
        // is one nobody will ever see, and holding it back would only show it
        // late.
        let mut speed = None;
        let mut level = None;
        let mut latest = None;
        let mut pose = None;
        let mut warnings = Vec::new();
        let mut scrolls = Vec::new();
        while let Some(ready) = self.pump.poll() {
            speed = Some(ready.speed);
            level = Some(ready.level);
            if let Some(pixels) = ready.pixels {
                latest = Some((pixels, ready.size));
            }
            // The render thread already throttles and dedupes this (see
            // `pump::due_pose`), so whichever of possibly several drained
            // messages carries one is the one worth keeping.
            if ready.pose.is_some() {
                pose = ready.pose;
            }
            // Unthrottled, unlike `pose` above: this is what a click
            // unprojects against, and a stale camera picks whatever was under
            // the cursor a fifth of a second ago.
            if ready.view.is_some() {
                self.view = ready.view;
            }
            if let Some(meshes) = ready.meshes {
                self.meshes = meshes;
            }
            warnings.extend(ready.warnings);
            scrolls.extend(ready.scrolls);
        }

        self.show_speed(speed, now, cx);
        if let Some(level) = level.filter(|level| *level != self.level) {
            self.level = level;
            cx.notify();
        }
        if let Some((pixels, size)) = latest {
            self.show_frame(pixels, size, window, cx);
            // A frame is up, so the overlay it carries has been laid out:
            // the injected wheel can find a frame under it from here on.
            if let Some((at, wheel)) = self.debug_wheel.take() {
                self.pump.wheel(at, wheel, wheel.notches);
            }
        }
        for scroll in scrolls {
            cx.emit(ViewportAction::Scrolled(scroll));
        }
        if let Some(pose) = pose {
            cx.emit(PoseSynced(pose));
        }
        if !warnings.is_empty() {
            cx.emit(AssetWarnings(warnings));
        }

        // Throttled to at most once per `interval` (see `hover::due`): a raw
        // OS mouse-move can fire far more often than the display refreshes,
        // and `Shell::hover_in_viewport` pays for a full-scene raycast every
        // time this resolves, so recording the position on every move but
        // only casting the ray here keeps that cost tied to frames drawn
        // rather than input events reported.
        if let Some((position, modifiers)) = self.hover_pending.take() {
            let elapsed = self
                .hover_resolved_at
                .map(|at| now.saturating_duration_since(at));
            if hover::due(elapsed, self.interval) {
                let scale = window.scale_factor();
                self.hover_moved(position, modifiers, scale, cx);
                self.hover_resolved_at = Some(now);
            } else {
                self.hover_pending = Some((position, modifiers));
            }
        }
        // Same gate as the hover above: the latest cursor position wins,
        // once per frame — see the `on_mouse_move` handler in `render`.
        if self.drag_pending.is_some() {
            let elapsed = self
                .drag_stepped_at
                .map(|at| now.saturating_duration_since(at));
            if hover::due(elapsed, self.interval) {
                self.step_drag(window, cx);
                self.drag_stepped_at = Some(now);
            }
        }

        (self.interval / POLLS_PER_FRAME).saturating_sub(now.elapsed())
    }

    fn show_frame(
        &mut self,
        pixels: Vec<u8>,
        size: (u32, u32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        if let Some(previous) = self.frame.take() {
            // Every image GPUI has drawn stays in the window's sprite atlas
            // until it is dropped: at 60 fps the atlas would grow by 300 MB/s.
            cx.drop_image(previous, Some(window));
        }

        self.frame = render_image(pixels, size.0, size.1);
        cx.notify();
        self.pump.stats().uploaded(started.elapsed());
    }

    /// Puts the flight speed on screen for a moment after the wheel changes it,
    /// where the standalone viewer puts it in its title bar.
    fn show_speed(&mut self, speed: Option<f32>, now: Instant, cx: &mut Context<Self>) {
        let changed = speed.filter(|speed| (speed - self.speed).abs() > f32::EPSILON);
        if let Some(speed) = changed {
            self.speed = speed;
            self.speed_shown_until = Some(now + SPEED_LABEL);
            cx.notify();
        } else if self.speed_shown_until.is_some_and(|until| now >= until) {
            self.speed_shown_until = None;
            cx.notify();
        }
    }

    fn begin_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.cursor = None;
        self.looking = true;
        self.pump.input(CameraInput::LookButton(true));
        // Orbiting the camera would otherwise leave whatever was last
        // hovered stuck on screen for the whole gesture: `mouse_moved` feeds
        // this same motion to the camera instead of resolving a new hover
        // while a look is in progress (see `hover::suppressed`, and
        // `self.looking` above rather than `self.lock.holds()` — the lock
        // never actually engages on Wayland), so nothing else would clear it.
        self.hover_pending = None;
        cx.emit(ViewportAction::Hover {
            ray: None,
            alt: false,
        });

        if let (Some(id), Some(centre)) = (
            pointer_lock::window_id(window),
            self.viewport.get().centre(),
        ) {
            self.lock.hold(id, centre);
        }
    }

    /// Ends the look however it ended — button up inside or outside the panel,
    /// lost focus, window deactivated — because each of those otherwise leaves
    /// the cursor hidden with the camera still turning.
    fn end_look(&mut self) {
        self.lock.release();
        self.cursor = None;
        self.looking = false;
        self.pump.input(CameraInput::LookButton(false));
    }

    /// Reports the motion since the last move. Whether it turns the view is the
    /// camera's call, not this one's: it only looks while the look button is
    /// held, and drains everything else so a move that preceded the press can
    /// never make it jump.
    fn mouse_moved(&mut self, position: Point<Pixels>) {
        if self.lock.holds() {
            if let Some((dx, dy)) = self.lock.moved() {
                self.pump.input(CameraInput::MouseLook { dx, dy });
            }
            return;
        }

        let Some(previous) = self.cursor.replace(position) else {
            return;
        };

        self.pump.input(CameraInput::MouseLook {
            dx: f32::from(position.x - previous.x),
            dy: f32::from(position.y - previous.y),
        });
    }

    fn key(&mut self, keystroke: &Keystroke, pressed: bool, cx: &mut Context<Self>) {
        let layout = Layout::of(cx.keyboard_layout().name());
        // Only on the press: a tool switch is an edge, not a state the way the
        // camera's own movement keys are.
        if pressed {
            let key = tool_key(&keystroke.key, layout);
            if let Some(action) = transform::action_for(key, keystroke.modifiers) {
                cx.emit(ViewportAction::Tool(action));
                return;
            }
            // Only while a part is actually held by its body, and only then:
            // with nothing in hand these are ordinary keys, and swallowing
            // them would take `r` away from whatever binds it next.
            if self.turn_key(&keystroke.key, keystroke.modifiers, cx) {
                return;
            }
        }

        // A chord — Ctrl+Z, Ctrl+S, Ctrl+Y — is a command for whichever
        // handler up the tree binds it, never a camera key: `z` sits on the
        // W position of an AZERTY keyboard, and undo must not also fly the
        // camera forward. Releases are always honoured, so a key pressed
        // plain and released with a modifier already down cannot leave the
        // camera moving on its own.
        if pressed && chorded(keystroke.modifiers) {
            return;
        }
        if let Some(key) = camera_key(&keystroke.key, layout) {
            self.pump.input(CameraInput::Key { key, pressed });
        }
    }

    /// Switches transform tool, or its world/local orientation — pushed down
    /// from the toolbar, which owns the choice.
    pub(crate) fn set_transform(&mut self, transform: Transform) {
        if transform == self.transform {
            return;
        }

        self.transform = transform;
        self.drag = None;
        self.pump.gizmo(transform.gizmo());
    }

    /// Where every selected part stands now: after a selection change, and
    /// after any edit that moved or resized one of them.
    pub(crate) fn set_targets(&mut self, targets: Targets) {
        // Never mid-gesture: the drag's own running answer is ahead of
        // whatever round trip through the DOM is landing now, and taking this
        // one would snap the parts back a frame.
        if self.drag.is_none() {
            self.targets = targets;
        }
    }

    /// Switches mode at runtime. The render thread owns the viewer, so the switch
    /// happens there, between two frames.
    pub(crate) fn set_quality(&mut self, mode: QualityLevel, cx: &mut Context<Self>) {
        if mode == self.quality {
            return;
        }

        self.quality = mode;
        self.pump.quality(mode);
        cx.notify();
    }

    /// Swaps the main camera between perspective and orthographic (parallel)
    /// projection at runtime, the same way `set_quality` above switches
    /// levels: the render thread owns the viewer, so the switch happens
    /// there, between two frames.
    pub(crate) fn set_orthographic(&mut self, orthographic: bool, cx: &mut Context<Self>) {
        if orthographic == self.orthographic {
            return;
        }

        self.orthographic = orthographic;
        self.pump.orthographic(orthographic);
        cx.notify();
    }

    /// Turns the corner label's frame-rate readout on or off — see
    /// `Shell::set_stats_shown`. Pure UI-thread state, unlike quality or
    /// projection above: `pump.stats()` is already updated by the render
    /// thread regardless, so nothing about what it draws needs to change.
    pub(crate) fn set_stats_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown == self.stats_shown {
            return;
        }

        self.stats_shown = shown;
        cx.notify();
    }

    /// Switches which unfocused preset `pacing` caps the render loop to —
    /// see `Shell::set_unfocused_fps`. No visible effect while focused, so
    /// unlike `set_quality`/`set_orthographic` this never needs `cx.notify`.
    pub(crate) fn set_unfocused_fps(&mut self, unfocused: pacing::UnfocusedFps) {
        if self.pacing.set_unfocused(unfocused) {
            self.retarget_pacing();
        }
    }

    /// Any keyboard or mouse input reaching the viewport counts as regaining
    /// the user's attention — see `pacing::FocusPacing::mark_input`. Called
    /// from every input handler in `render` below, so the throttle from
    /// before an unfocused window's activation event lands doesn't also
    /// delay the very input that's supposed to end it.
    fn note_input(&mut self) {
        if self.pacing.mark_input() {
            self.retarget_pacing();
        }
    }

    /// Recomputes the render loop's target interval from the current focus
    /// state and pushes it to both halves of the loop: `self.interval` (read
    /// by `advance`'s own poll delay and the hover/drag gates) and the render
    /// thread itself (the actual throttle — see `Pump::set_interval`).
    fn retarget_pacing(&mut self) {
        let next = self.pacing.interval(self.full_interval);
        if next != self.interval {
            self.interval = next;
            self.pump.set_interval(next);
        }
    }

    /// What the corner label reads: the speed is passed only while its moment
    /// on screen lasts, the frame rate only while the Stats toggle is on —
    /// and `0.0` (no full second counted yet) reads the same as off.
    fn status_label(&self) -> SharedString {
        let speed = self.speed_shown_until.map(|_| self.speed);
        let fps = self
            .stats_shown
            .then(|| self.pump.stats().latest_fps())
            .filter(|fps| *fps > 0.0);
        label::status(self.quality, self.level, fps, speed)
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only called while the dock actually mounts this panel — see
        // `advance` and the `painted` field.
        self.painted.set(true);
        let viewport = self.viewport.clone();
        let scale = window.scale_factor();
        // A frame larger than the window itself would be read back only to be
        // scaled down again.
        let cap = window.viewport_size();
        let cap = (
            device_pixels(cap.width, scale),
            device_pixels(cap.height, scale),
        );

        div()
            // Only `on_hover` (used below, to clear the hover outline when
            // the cursor leaves the panel) actually needs an id — it is
            // `StatefulInteractiveElement`'s alone, unlike every other
            // handler here.
            .id("workspace-viewport")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(rgb(0x1c1d20))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    view.note_input();
                    window.focus(&view.focus, cx);
                    let scale = window.scale_factor();
                    view.press(event.position, event.modifiers, scale, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, window, cx| view.end_drag(window, cx)),
            )
            // A drag released off the panel still ends it, or the part would
            // keep following the cursor with nothing to let go of it.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, window, cx| view.end_drag(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    view.note_input();
                    view.begin_look(window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|view, _: &MouseUpEvent, _, _| {
                    view.end_look();
                }),
            )
            // A drag released outside the panel still ends the look, or the
            // button would stay logically held with nothing to release it.
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|view, _: &MouseUpEvent, _, _| {
                    view.end_look();
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, _| {
                view.note_input();
                // Recorded, not applied: the step itself runs in `advance`,
                // at most once per frame (see `gizmo::Drag`'s doc and
                // `WorkspaceView::step_drag`), the way a hover is. A mouse
                // reports far more moves than the display draws, and each
                // step writes every carried part into the DOM and reflects
                // it — for a large Model, milliseconds the UI thread cannot
                // spend a thousand times a second.
                if view.dragging() {
                    view.drag_pending = Some((event.position, event.modifiers));
                    return;
                }
                view.mouse_moved(event.position);
                // While a look is in progress this same motion just turned
                // the camera above, not the cursor: there is nothing new
                // under it to resolve a hover against, and the last one
                // already stands cleared (see `begin_look`). Gated on
                // `looking`, not `view.lock.holds()` — the lock never
                // actually engages on Wayland (see `hover::suppressed`).
                // Recording the position is cheap; the raycast itself is
                // throttled in `advance` (see `hover::due`), not run here.
                if !hover::suppressed(view.looking) {
                    view.hover_pending = Some((event.position, event.modifiers));
                }
            }))
            // The cursor leaving the panel altogether never fires another
            // `on_mouse_move` to say so — bounds-scoped, like every handler
            // above — so a stale hover box would otherwise outlive it; `false`
            // is exactly that transition (see `Interactivity::on_hover`).
            .on_hover(cx.listener(|view, hovering: &bool, _, cx| {
                if !hovering {
                    view.hover_pending = None;
                    cx.emit(ViewportAction::Hover {
                        ray: None,
                        alt: false,
                    });
                }
            }))
            // Not straight to the camera: a `ScrollingFrame` under the
            // cursor takes the notch first (see `scroll`).
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, _| {
                view.note_input();
                let scale = window.scale_factor();
                view.wheel(event.position, event.delta, event.modifiers.shift, scale);
            }))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                view.note_input();
                view.key(&event.keystroke, true, cx);
            }))
            .on_key_up(cx.listener(|view, event: &KeyUpEvent, _, cx| {
                view.key(&event.keystroke, false, cx);
            }))
            // Shift alone never arrives as a keystroke: modifiers are reported
            // on their own, and Shift is Studio's precision modifier.
            .on_modifiers_changed(cx.listener(|view, event: &ModifiersChangedEvent, _, _| {
                view.note_input();
                view.pump.input(CameraInput::Key {
                    key: rbx_viewer::CameraKey::Slow,
                    pressed: event.modifiers.shift,
                });
                // Alt toggles what a click would land on (the whole model, or
                // one part inside it), so the "about to click" outline has to
                // re-resolve the moment Alt is pressed or released, without
                // waiting for the cursor to move. Re-queued at the last known
                // position; suppressed mid-look exactly as an ordinary move is.
                if !hover::suppressed(view.looking) {
                    if let Some(position) = view.cursor {
                        view.hover_pending = Some((position, event.modifiers));
                    }
                }
            }))
            .when_some(self.frame.clone(), |this, frame| {
                this.child(img(frame).size_full().object_fit(ObjectFit::Fill))
            })
            // Nothing is painted here: the canvas is only how an element's laid
            // out bounds reach the render loop and the pointer lock.
            .child(
                canvas(
                    move |bounds, _, _| {
                        viewport.set(Viewport {
                            origin: (
                                device_pixels(bounds.origin.x, scale),
                                device_pixels(bounds.origin.y, scale),
                            ),
                            size: (
                                device_pixels(bounds.size.width, scale).min(cap.0),
                                device_pixels(bounds.size.height, scale).min(cap.1),
                            ),
                        });
                    },
                    |_, _: (), _, _| {},
                )
                .absolute()
                // Pinned rather than left to `absolute`'s default: with no inset
                // an absolute child takes its static position, which here is
                // below the image it follows — and a viewport whose origin is a
                // panel too low is one the pointer lock pins the cursor outside
                // of.
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .px_2()
                    .py_0p5()
                    .bg(rgba(0x14151ae0))
                    .text_xs()
                    .text_color(rgb(0xe4e5e9))
                    .child(self.status_label()),
            )
    }
}
