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

mod frame;
mod gizmo;
mod input;
mod label;
mod presence;
mod pump;
mod quality;
mod reload;
mod stats;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Mat3, Mat4, Vec3};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::pick::{Meshes, Ray};
use rbx_viewer::{CameraInput, Headless, Pose, QualityLevel};

use crate::camera::PlaceCamera;
use crate::pointer_lock::{self, PointerLock};
use crate::settle::Settle;
use crate::transform::{self, Targets, Transform};
use crate::{display, pacing};
use frame::{device_pixels, render_image, Viewport};
use gizmo::Drag;
use input::{camera_key, wheel_notches, Layout};
use pump::Pump;

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
    Pick {
        ray: Ray,
        cycling: bool,
        extend: bool,
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
        referent: Ref,
        size: Vec3,
        position: Vec3,
        first: bool,
    },
    /// A Rotate drag turned the part about its centre, which is where the
    /// rings stand. Only the `CFrame`'s rotation changes.
    Rotated {
        referent: Ref,
        orientation: Mat3,
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
}

impl EventEmitter<ViewportAction> for WorkspaceView {}

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
    /// One display refresh: the budget a frame is given.
    interval: Duration,
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
    /// Whether the drag in progress has actually moved the part yet, which is
    /// what tells `Shell` which move opens the gesture's one undo step.
    dragged: bool,
    /// Kept only to stay subscribed: dropping these unregisters the listeners.
    _subscriptions: [Subscription; 2],
}

impl WorkspaceView {
    pub(crate) fn new(
        mut viewer: Headless,
        camera: Option<PlaceCamera>,
        quality: QualityLevel,
        orthographic: bool,
        selected: Option<Ref>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // A file with no camera of its own still has to be shown somehow: the
        // viewer then orbits its bounds until the first input, exactly as
        // `rbxview --orbit` does.
        if let Some(camera) = camera {
            viewer.open_at(camera.eye, camera.look_at, camera.fov_degrees);
        }
        viewer.set_selection(&Vec::from_iter(selected));
        viewer.set_orthographic(orthographic);

        let interval = pacing::frame_interval(display::refresh_hz());
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
        // activation is watched too.
        let deactivated = cx.observe_window_activation(window, |view, window, _| {
            if !window.is_window_active() {
                view.end_look();
            }
        });

        WorkspaceView {
            pump: Pump::spawn(viewer, interval, quality),
            focus,
            cursor: None,
            lock: PointerLock::new(),
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
            interval,
            speed,
            speed_shown_until: None,
            quality,
            level: QualityLevel::MAX,
            orthographic,
            transform: Transform::default(),
            targets: Targets::default(),
            neighbours: Vec::new(),
            view: None,
            meshes: Meshes::default(),
            drag: None,
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

    /// Puts whatever the render thread has finished on screen, and returns how
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
        }

        self.show_speed(speed, now, cx);
        if let Some(level) = level.filter(|level| *level != self.level) {
            self.level = level;
            cx.notify();
        }
        if let Some((pixels, size)) = latest {
            self.show_frame(pixels, size, window, cx);
        }
        if let Some(pose) = pose {
            cx.emit(PoseSynced(pose));
        }
        if !warnings.is_empty() {
            cx.emit(AssetWarnings(warnings));
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

    fn begin_look(&mut self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
        self.cursor = None;
        self.pump.input(CameraInput::LookButton(true));

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
        // Only on the press: a tool switch is an edge, not a state the way the
        // camera's own movement keys are.
        if pressed {
            if let Some(action) = transform::action_for(&keystroke.key, keystroke.modifiers) {
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

        let layout = Layout::of(cx.keyboard_layout().name());
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

    /// What the corner label reads: the speed is passed only while its moment
    /// on screen lasts.
    fn status_label(&self) -> SharedString {
        let speed = self.speed_shown_until.map(|_| self.speed);
        label::status(self.quality, self.level, speed)
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
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(rgb(0x1c1d20))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    window.focus(&view.focus, cx);
                    let scale = window.scale_factor();
                    view.press(event.position, event.modifiers, scale, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, _| view.end_drag()),
            )
            // A drag released off the panel still ends it, or the part would
            // keep following the cursor with nothing to let go of it.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, _| view.end_drag()),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
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
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, window, cx| {
                if view.dragging() {
                    let scale = window.scale_factor();
                    view.drag_to(event.position, event.modifiers, scale, cx);
                    return;
                }
                view.mouse_moved(event.position);
            }))
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, _, _| {
                let notches = wheel_notches(event.delta);
                view.pump.input(CameraInput::Wheel { notches });
            }))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                view.key(&event.keystroke, true, cx);
            }))
            .on_key_up(cx.listener(|view, event: &KeyUpEvent, _, cx| {
                view.key(&event.keystroke, false, cx);
            }))
            // Shift alone never arrives as a keystroke: modifiers are reported
            // on their own, and Shift is Studio's precision modifier.
            .on_modifiers_changed(cx.listener(|view, event: &ModifiersChangedEvent, _, _| {
                view.pump.input(CameraInput::Key {
                    key: rbx_viewer::CameraKey::Slow,
                    pressed: event.modifiers.shift,
                });
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
