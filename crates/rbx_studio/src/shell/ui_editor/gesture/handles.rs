//! The handles a selection grows besides its frame's — each corner's
//! radius — what a drag inside a list or grid does instead of moving, and
//! the right-click menu.

use gpui_kit::*;
use rbx_viewer::GuiBox;

use super::super::super::Shell;
use super::super::inspector::{Key, CORNERS};
use super::super::is_gui_object;
use super::{Gesture, Held};
use crate::ui_canvas::{self, rotate, Rect};

/// How far in from its corner, in panel pixels, a square corner's radius
/// handle sits — clear of the resize handle on the corner itself.
const RADIUS_INSET: f32 = 12.0;
/// How small on screen an element's shorter side can get before its radius
/// handles would crowd the resize handles out.
const RADIUS_ROOM: f32 = 64.0;

impl Shell {
    /// A right-click: the Explorer's own context menu, on what is under the
    /// pointer — picked first unless it is part of the selection already —
    /// or on the screen itself.
    pub(in crate::shell::ui_editor) fn canvas_menu(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let point = self.ui.view.to_canvas(self.panel_point(event.position));
        let target = ui_canvas::hit(&boxes, point).unwrap_or(root.referent);
        self.open_row_menu(target, event.position, cx);
    }

    /// The first selected element's corners' radii, in pixels, clockwise
    /// from the top left.
    pub(in crate::shell::ui_editor) fn radii(&self) -> [f32; 4] {
        CORNERS.map(|key| match self.reading(key) {
            Some(Some(value)) => value.first().copied().unwrap_or(0.0),
            _ => 0.0,
        })
    }

    /// Whether the radius handles are up: one element selected, big enough
    /// on screen for them, and no other gesture under way.
    pub(in crate::shell::ui_editor) fn radius_shown(&self) -> bool {
        let [only] = self.selected_all() else {
            return false;
        };
        let idle = matches!(
            self.ui.gesture,
            None | Some(Gesture::Pressed { .. }) | Some(Gesture::Radius { .. })
        );
        idle && is_gui_object(&self.dom, &self.database, *only)
    }

    /// What a drag of `held` reorders instead of moving: its one element,
    /// when a list or grid lays it out — `Position` means nothing there.
    pub(super) fn reorder_of(
        &self,
        held: &[Held],
        root: &GuiBox,
        boxes: &[GuiBox],
        to: [f32; 2],
    ) -> Option<Gesture> {
        let [one] = held else {
            return None;
        };
        let shown = self.shown_layout(root, boxes)?;
        if !shown.children.iter().any(|(r, _)| *r == one.referent) {
            return None;
        }
        let others = shown
            .children
            .into_iter()
            .filter(|(r, _)| *r != one.referent)
            .collect();
        Some(Gesture::Reorder {
            referent: one.referent,
            layout: shown.layout,
            others,
            grid: shown.grid,
            axis: shown.axis,
            to,
        })
    }
}

/// Where each corner's radius handle stands on `rect` turned `degrees`:
/// its radius in from the corner along both sides, or `RADIUS_INSET` panel
/// pixels while the corner is square or tighter than that.
pub(in crate::shell::ui_editor) fn radius_handles(
    rect: &Rect,
    degrees: f32,
    radii: [f32; 4],
    zoom: f32,
) -> Vec<([i8; 2], [f32; 2])> {
    if rect.w.min(rect.h) * zoom < RADIUS_ROOM {
        return Vec::new();
    }
    let c = rect.centre();
    SIDES
        .into_iter()
        .zip(radii)
        .map(|(corner, radius)| {
            let inset = radius
                .min(rect.w.min(rect.h) * 0.5)
                .max(RADIUS_INSET / zoom);
            let local = [0, 1].map(|axis| {
                let half = [rect.w, rect.h][axis] * 0.5;
                f32::from(corner[axis]) * (half - inset)
            });
            let [x, y] = rotate(local, degrees);
            (corner, [c[0] + x, c[1] + y])
        })
        .collect()
}

/// Which side of the box each corner is on, clockwise from the top left.
const SIDES: [[i8; 2]; 4] = [[-1, -1], [1, -1], [1, 1], [-1, 1]];

/// The field for `corner`'s own radius.
pub(super) fn corner_key(corner: [i8; 2]) -> Key {
    let index = SIDES.iter().position(|&side| side == corner).unwrap_or(0);
    CORNERS[index]
}

/// The radius a corner's handle dragged to `point` asks for: how far in
/// from `corner` it is, along both sides on average, up to the round end.
pub(in crate::shell::ui_editor) fn radius_at(
    rect: &Rect,
    degrees: f32,
    corner: [i8; 2],
    point: [f32; 2],
) -> f32 {
    let c = rect.centre();
    let local = rotate([point[0] - c[0], point[1] - c[1]], -degrees);
    let inward =
        [0, 1].map(|axis| [rect.w, rect.h][axis] * 0.5 - f32::from(corner[axis]) * local[axis]);
    ((inward[0] + inward[1]) * 0.5).clamp(0.0, rect.w.min(rect.h) * 0.5)
}
