//! The plugins' `Handles`, `ArcHandles` and `*HandleAdornment`s as the
//! cursor meets them: a press on one starts a plugin drag ahead of every
//! tool, and its moves, release and hover go back to the plugins as
//! `ViewportAction::PluginHandle` rather than touching the place.

use glam::{Mat4, Vec3};
use gpui_kit::*;
use rbx_dom::{Instance, Ref};
use rbx_lua::plugin::{HandleEvent, HandleEventKind};
use rbx_viewer::gizmo::{along_axis, angle_step, ring_crossing};
use rbx_viewer::handle_pick::{self, Part, Shape};
use rbx_viewer::pick::Ray;

use super::{ViewportAction, WorkspaceView};

#[derive(Default)]
pub(super) struct PluginHandles {
    shapes: Vec<Shape>,
    drag: Option<Grab>,
    hovered: Option<(Ref, Part)>,
}

/// A handle held down: what was grabbed, the adornee frame it is measured
/// against (as of the grab, so the plugin moving the part does not shift
/// the ruler under the drag), and where the cursor started.
struct Grab {
    referent: Ref,
    part: Part,
    frame: Mat4,
    start: f32,
    radius: f32,
    angle: f32,
    turned: f32,
}

fn event(referent: Ref, kind: HandleEventKind) -> ViewportAction {
    ViewportAction::PluginHandle(HandleEvent { referent, kind })
}

fn press_kind(part: Part) -> HandleEventKind {
    match part {
        Part::Face(face) => HandleEventKind::Down { face },
        Part::Axis(axis) => HandleEventKind::ArcDown { axis },
        Part::Body => HandleEventKind::AdornmentDown,
    }
}

fn release_kind(part: Part) -> HandleEventKind {
    match part {
        Part::Face(face) => HandleEventKind::Up { face },
        Part::Axis(axis) => HandleEventKind::ArcUp { axis },
        Part::Body => HandleEventKind::AdornmentUp,
    }
}

fn enter_kind(part: Part) -> HandleEventKind {
    match part {
        Part::Face(face) => HandleEventKind::Enter { face },
        Part::Axis(axis) => HandleEventKind::ArcEnter { axis },
        Part::Body => HandleEventKind::AdornmentEnter,
    }
}

fn leave_kind(part: Part) -> HandleEventKind {
    match part {
        Part::Face(face) => HandleEventKind::Leave { face },
        Part::Axis(axis) => HandleEventKind::ArcLeave { axis },
        Part::Body => HandleEventKind::AdornmentLeave,
    }
}

fn face_axis(frame: Mat4, face: u8) -> (Vec3, Vec3) {
    let origin = frame.transform_point3(Vec3::ZERO);
    let axis = frame
        .transform_vector3(handle_pick::face_normal(face))
        .normalize_or_zero();
    (origin, axis)
}

fn ring_axes(frame: Mat4, axis: u8) -> (Vec3, (Vec3, Vec3, Vec3)) {
    let i = axis as usize;
    let column = |k: usize| frame.col(k).truncate().normalize_or_zero();
    (
        frame.transform_point3(Vec3::ZERO),
        (column(i), column((i + 1) % 3), column((i + 2) % 3)),
    )
}

impl WorkspaceView {
    /// The plugins' adornments to draw and the shapes the cursor can grab.
    pub(crate) fn set_plugin_handles(&mut self, adornments: Vec<Instance>, shapes: Vec<Shape>) {
        self.pump.plugin_adornments(adornments);
        let handles = &mut self.plugin_handles;
        if let Some((referent, _)) = handles.hovered {
            if !shapes.iter().any(|shape| shape.referent == referent) {
                handles.hovered = None;
            }
        }
        if let Some(grab) = &handles.drag {
            if !shapes.iter().any(|shape| shape.referent == grab.referent) {
                handles.drag = None;
            }
        }
        handles.shapes = shapes;
    }

    /// A left press: grabs a plugin handle under the cursor, if any. `true`
    /// when one took the press.
    pub(super) fn plugin_press(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.plugin_handles.shapes.is_empty() {
            return false;
        }
        let Some(ray) = self.cursor_ray(position, scale) else {
            return false;
        };
        let Some(shape) = handle_pick::pick_shape(&self.plugin_handles.shapes, ray).copied() else {
            return false;
        };
        let mut grab = Grab {
            referent: shape.referent,
            part: shape.part,
            frame: shape.frame,
            start: 0.0,
            radius: 0.0,
            angle: 0.0,
            turned: 0.0,
        };
        match shape.part {
            Part::Face(face) => {
                let (origin, axis) = face_axis(shape.frame, face);
                grab.start = along_axis(origin, axis, ray).unwrap_or(0.0);
            }
            Part::Axis(axis) => {
                let (origin, plane) = ring_axes(shape.frame, axis);
                if let Some((angle, radius, _)) = ring_crossing(origin, plane, ray) {
                    grab.angle = angle;
                    grab.radius = radius;
                }
            }
            Part::Body => {}
        }
        cx.emit(event(shape.referent, press_kind(shape.part)));
        self.plugin_handles.drag = Some(grab);
        true
    }

    /// Cursor motion: a held handle's drag step, or hover Enter/Leave. `true`
    /// while a plugin drag holds the cursor.
    pub(super) fn plugin_move(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.plugin_handles.shapes.is_empty() && self.plugin_handles.drag.is_none() {
            return false;
        }
        let Some(ray) = self.cursor_ray(position, scale) else {
            return self.plugin_handles.drag.is_some();
        };
        if let Some(grab) = self.plugin_handles.drag.as_mut() {
            if let Some(kind) = drag_step(grab, ray) {
                cx.emit(event(grab.referent, kind));
            }
            return true;
        }
        let now = handle_pick::pick(&self.plugin_handles.shapes, ray);
        let before = self.plugin_handles.hovered;
        if now != before {
            if let Some((referent, part)) = before {
                cx.emit(event(referent, leave_kind(part)));
            }
            if let Some((referent, part)) = now {
                cx.emit(event(referent, enter_kind(part)));
            }
            self.plugin_handles.hovered = now;
        }
        false
    }

    /// A left release: ends a plugin drag. `true` when one was held.
    pub(super) fn plugin_release(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(grab) = self.plugin_handles.drag.take() else {
            return false;
        };
        cx.emit(event(grab.referent, release_kind(grab.part)));
        true
    }
}

fn drag_step(grab: &mut Grab, ray: Ray) -> Option<HandleEventKind> {
    match grab.part {
        Part::Face(face) => {
            let (origin, axis) = face_axis(grab.frame, face);
            let along = along_axis(origin, axis, ray)?;
            Some(HandleEventKind::Drag {
                face,
                distance: along - grab.start,
            })
        }
        Part::Axis(axis) => {
            let (origin, plane) = ring_axes(grab.frame, axis);
            let (angle, radius, _) = ring_crossing(origin, plane, ray)?;
            grab.turned += angle_step(grab.angle, angle);
            grab.angle = angle;
            Some(HandleEventKind::ArcDrag {
                axis,
                angle: grab.turned,
                delta_radius: radius - grab.radius,
            })
        }
        Part::Body => None,
    }
}
