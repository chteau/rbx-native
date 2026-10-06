//! The frame sheet: one `ViewportFrame`'s own editing surface, a sheet over
//! the canvas — its contents drawn large through the frame's own camera, a
//! camera to fly, orbit and pan, framing, the frame's lighting, and models
//! and parts cloned in from `Workspace`.
//!
//! A sheet in the main window rather than a window of its own: the lighting
//! rows are the design panel's own fields, built in this window's context,
//! and the picture comes off the canvas's render thread with the canvas.
//!
//! Like the rest of the canvas it holds no copy of the place. The pose is
//! read out of the frame's camera every step and written straight back
//! (`crate::viewport_frame`), through the one history: a fit, an insert, a
//! removal or a typed field is one undo step, and so is a whole flight —
//! its first step pushes (creating and wiring the `Camera` if the frame has
//! none) and every later one folds into that entry, the way a canvas drag
//! does (`Shell::write_drag_after`).

mod input;
mod kit;
mod panel;
mod picker;
mod view;

pub(super) use kit::{separator, text_button, Look};

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use glam::Vec3;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::{Change, Ref};
use rbx_viewer::{CameraFeel, Flight, Pose};

use super::Shell;
use crate::viewport_frame::{self as frame, Gesture, Step, FRAME_CLASS, THREE_QUARTER};
use crate::workspace_view::FrameRequest;

/// Read once at startup: `1` opens the sheet on the selected
/// `ViewportFrame`, `picker`/`picker:<query>`/`fit` with one of its
/// popovers up — what a capture of each state needs, documented in
/// `main`'s module doc comment.
const FRAME_SHEET_VARIABLE: &str = "RBX_STUDIO_FRAME_SHEET";

/// What a left drag on the stage does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tool {
    Orbit,
    Hand,
}

/// Which popover is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Popover {
    Picker,
    Fit,
}

/// One open sheet. Nothing here is the place: which frame, how the camera
/// is being steered, and the sheet's own view settings.
pub(super) struct FrameSheet {
    pub(super) frame: Ref,
    flight: Flight,
    gesture: Gesture,
    /// The log of the step that opened the gesture in flight — the
    /// `Camera` it created, when it did — kept at the head of every later
    /// step's log so the one entry still undoes it.
    opened: Vec<Change>,
    /// The history's revision just after the gesture's opening step: any
    /// other edit or an undo in between moves it, and the gesture then
    /// opens a step of its own rather than writing into someone else's.
    revision: u64,
    /// When the flight last stepped; `None` while nothing steers.
    ticked: Option<Instant>,
    pub(super) tool: Tool,
    /// The left drag in flight, and where the pointer last was — for the
    /// look button's mouse-look as well.
    pointer: Option<Point<Pixels>>,
    dragging: bool,
    /// Whether the frame's own background shows behind the scene.
    pub(super) background: bool,
    pub(super) popover: Option<Popover>,
    pub(super) query: Entity<InputState>,
    /// Where the stage area was laid out, in window pixels: what the
    /// picture's size is fitted to.
    pub(super) area: Rc<Cell<Bounds<Pixels>>>,
    /// The frame drawn alone, as the canvas request asks for it — set while
    /// rendering, from the area and the frame's own size.
    pub(super) request: Cell<Option<FrameRequest>>,
    pub(super) focus: FocusHandle,
    pub(super) fov: Entity<InputState>,
    _subscriptions: [Subscription; 2],
}

impl Shell {
    /// The `ViewportFrame` the selection is, alone — what the Edit button
    /// and a double-click open the sheet on.
    pub(super) fn selected_viewport_frame(&self) -> Option<Ref> {
        match self.selected_all() {
            [only] => self
                .dom
                .get(*only)
                .filter(|instance| self.database.is_subclass_of(instance.class(), FRAME_CLASS))
                .map(|_| *only),
            _ => None,
        }
    }

    pub(super) fn open_frame_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.selected_viewport_frame() else {
            return;
        };
        let speed = frame::contents(&self.dom, &self.database, target)
            .map_or(10.0, |(_, radius)| radius * 1.5);
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search the Workspace"));
        let fov = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = [
            cx.observe(&query, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &fov,
                window,
                |shell, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let text = input.read(cx).value().to_string();
                        shell.commit_frame_fov(&text, window, cx);
                    }
                },
            ),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.ui.sheet = Some(FrameSheet {
            frame: target,
            flight: Flight::new(speed, CameraFeel::default()),
            gesture: Gesture::default(),
            opened: Vec::new(),
            revision: 0,
            ticked: None,
            tool: Tool::Orbit,
            pointer: None,
            dragging: false,
            background: true,
            popover: None,
            query,
            area: Rc::default(),
            request: Cell::new(None),
            focus,
            fov,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    pub(super) fn close_frame_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.sheet.take().is_some() {
            self.focus_ui_canvas(window, cx);
            cx.notify();
        }
    }

    /// The sheet, while the frame it was opened on is still the selection
    /// and still there: an undo that takes the frame away, or a click
    /// elsewhere, closes it.
    pub(super) fn sync_frame_sheet(&mut self) {
        let open = self.ui.sheet.as_ref().map(|sheet| sheet.frame);
        if open.is_some() && open != self.selected_viewport_frame() {
            self.ui.sheet = None;
        }
    }

    /// The frame's pose as the DOM holds it, or — for a frame not yet
    /// looked through — a three-quarter fit of what it holds.
    pub(super) fn frame_pose(&self, target: Ref, cx: &App) -> Pose {
        frame::pose_of(&self.dom, &self.database, target)
            .unwrap_or_else(|| self.fitted(target, THREE_QUARTER, 70.0, cx))
    }

    /// `target`'s contents fitted from `aim`, or a view of the origin from
    /// ten studs off for an empty frame.
    fn fitted(&self, target: Ref, aim: (f32, f32), fov: f32, cx: &App) -> Pose {
        let sphere =
            frame::contents(&self.dom, &self.database, target).unwrap_or((Vec3::ZERO, 2.5));
        frame::fit(sphere, aim, fov, self.frame_aspect(target, cx))
    }

    /// The frame's own width over height, as last laid out on the canvas.
    pub(super) fn frame_aspect(&self, target: Ref, cx: &App) -> f32 {
        self.frame_size(target, cx)
            .map_or(16.0 / 9.0, |[width, height]| width / height.max(1.0))
    }

    /// The frame's `AbsoluteSize`, as the canvas last laid it out.
    pub(super) fn frame_size(&self, target: Ref, cx: &App) -> Option<[f32; 2]> {
        self.viewport
            .read(cx)
            .canvas()?
            .boxes
            .iter()
            .find(|found| found.referent == target)
            .map(|found| [found.rect[2], found.rect[3]])
    }

    /// One step of a camera gesture: the first pushes an undo entry (and
    /// makes the camera, if need be), every later one folds into it.
    fn write_frame_pose(&mut self, pose: Pose, step: Step, cx: &mut Context<Self>) {
        let Some((target, revision)) = self
            .ui
            .sheet
            .as_ref()
            .map(|sheet| (sheet.frame, sheet.revision))
        else {
            return;
        };
        let step = match step {
            Step::Continue if self.history.revision() != revision => Step::First,
            step => step,
        };
        match step {
            Step::First => {
                let opened = self.edit_gui_tree(
                    "viewport camera",
                    |dom, database| {
                        frame::write_pose(dom, database, target, pose);
                        (Vec::new(), None)
                    },
                    cx,
                );
                let revision = self.history.revision();
                if let Some(sheet) = &mut self.ui.sheet {
                    sheet.opened = opened;
                    sheet.revision = revision;
                }
            }
            Step::Continue => {
                frame::write_pose(&mut self.dom, &self.database, target, pose);
                let changes = self.dom.take_changes();
                self.reflect_changes(&changes, cx);
                let opened = self
                    .ui
                    .sheet
                    .as_ref()
                    .map_or_else(Vec::new, |sheet| sheet.opened.clone());
                self.record_history_change([opened, changes].concat());
                cx.notify();
            }
        }
    }

    /// A one-off camera edit — a fit, a reset, a typed field of view — as
    /// its own undo step, ending whatever gesture was in flight.
    fn set_frame_pose(&mut self, pose: Pose, cx: &mut Context<Self>) {
        if let Some(sheet) = &mut self.ui.sheet {
            sheet.gesture.close();
        }
        self.write_frame_pose(pose, Step::First, cx);
        if let Some(sheet) = &mut self.ui.sheet {
            sheet.gesture.close();
        }
    }

    /// Fit contents: the current view direction kept, backed off until
    /// everything in the frame is on the stage.
    pub(super) fn fit_frame(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let pose = self.frame_pose(target, cx);
        let fitted = self.fitted(target, (pose.yaw, pose.pitch), pose.fov_degrees, cx);
        self.set_frame_pose(fitted, cx);
    }

    /// Reset camera: the three-quarter view a fresh frame opens on, at the
    /// default field of view.
    pub(super) fn reset_frame_camera(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let pose = self.fitted(target, THREE_QUARTER, 70.0, cx);
        self.set_frame_pose(pose, cx);
    }

    /// Top view: the contents fitted from straight above, as near as the
    /// camera's pitch allows.
    pub(super) fn top_frame_view(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let fov = self.frame_pose(target, cx).fov_degrees;
        let pose = self.fitted(target, (0.0, 89f32.to_radians()), fov, cx);
        self.set_frame_pose(pose, cx);
    }

    /// The Camera section's field: degrees, refused outside what a
    /// `Camera` takes.
    fn commit_frame_fov(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let mut pose = self.frame_pose(target, cx);
        let typed = text.trim().trim_end_matches('°').trim().parse::<f32>();
        // `Camera.FieldOfView`'s own range.
        match typed {
            Ok(fov) if (1.0..=120.0).contains(&fov) && fov != pose.fov_degrees => {
                pose.fov_degrees = fov;
                self.set_frame_pose(pose, cx);
            }
            _ => self.show_frame_fov(window, cx),
        }
    }

    /// Puts the frame's field of view back in its field, unless it is
    /// being typed in.
    pub(super) fn show_frame_fov(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, input)) = self
            .ui
            .sheet
            .as_ref()
            .map(|sheet| (sheet.frame, sheet.fov.clone()))
        else {
            return;
        };
        let text = crate::shell::format_scrubbed(self.frame_pose(target, cx).fov_degrees);
        let stale = {
            let state = input.read(cx);
            state.value() != text.as_str() && !state.focus_handle(cx).is_focused(window)
        };
        if stale {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    /// Clones `source` into the frame, one undo step — and, for a frame not
    /// yet looked through, gives it a camera on what just landed in that
    /// same step, so the first insert is already on the stage.
    pub(super) fn insert_into_frame(&mut self, source: Ref, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let aspect = self.frame_aspect(target, cx);
        self.edit_gui_tree(
            "insert into the viewport",
            |dom, database| {
                super::super::clipboard::clone_into(dom, source, target);
                if frame::pose_of(dom, database, target).is_none() {
                    let sphere =
                        frame::contents(dom, database, target).unwrap_or((Vec3::ZERO, 2.5));
                    let pose = frame::fit(sphere, THREE_QUARTER, 70.0, aspect);
                    frame::write_pose(dom, database, target, pose);
                }
                (Vec::new(), None)
            },
            cx,
        );
        if let Some(sheet) = &mut self.ui.sheet {
            sheet.popover = None;
            sheet.gesture.close();
        }
    }

    /// Minus: the copy deleted, one undo step. The `Workspace` original was
    /// never touched.
    pub(super) fn remove_from_frame(&mut self, child: Ref, cx: &mut Context<Self>) {
        self.edit_gui_tree(
            "remove from the viewport",
            |dom, _| {
                dom.remove(child);
                (Vec::new(), None)
            },
            cx,
        );
    }

    /// `RBX_STUDIO_FRAME_SHEET`: see [`FRAME_SHEET_VARIABLE`].
    pub(in crate::shell) fn apply_debug_frame_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(spec) = std::env::var(FRAME_SHEET_VARIABLE) else {
            return;
        };
        if self.ui.sheet.is_none() {
            self.open_frame_sheet(window, cx);
        }
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        let (popover, query) = match spec.split_once(':') {
            Some((popover, query)) => (popover, query),
            None => (spec.as_str(), ""),
        };
        sheet.popover = match popover {
            "picker" => Some(Popover::Picker),
            "fit" => Some(Popover::Fit),
            _ => None,
        };
        if !query.is_empty() {
            let (input, query) = (sheet.query.clone(), query.to_owned());
            input.update(cx, |state, cx| state.set_value(query, window, cx));
        }
    }
}
