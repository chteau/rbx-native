//! One step of a drag in flight: where it moves what it holds, what it
//! snaps onto on the way, and the writes that follow.

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::snap::round_to;
use rbx_viewer::GuiBox;

use super::super::super::Shell;
use super::super::is_gui_object;
use super::{Gesture, Held, ROTATE_STEP, SNAP_REACH};
use crate::ui_canvas::guides;
use crate::ui_canvas::{
    angle_of, box_of, position_shift, resize, rotate, shifted, udim2_text, Rect,
};

impl Shell {
    /// Every selected element a move carries: the laid-out `GuiObject`s,
    /// less any whose ancestor is carried too — moving the parent already
    /// moves it.
    pub(in crate::shell::ui_editor) fn held_selection(&self, boxes: &[GuiBox]) -> Vec<Held> {
        let selected = self.selected_all();
        selected
            .iter()
            .filter(|&&referent| is_gui_object(&self.dom, &self.database, referent))
            .filter(|&&referent| {
                let mut up = self.dom.parent(referent);
                while let Some(ancestor) = up {
                    if selected.contains(&ancestor) {
                        return false;
                    }
                    up = self.dom.parent(ancestor);
                }
                true
            })
            .filter_map(|&referent| box_of(boxes, referent))
            .filter_map(|placed| Held::read(&self.dom, placed))
            .collect()
    }

    /// The boxes a drag of `held` snaps onto: its siblings and its parent —
    /// the screen's frame when the parent is the screen.
    fn snap_targets(&self, held: &[Held], cx: &App) -> Vec<Rect> {
        let Some((screen, boxes)) = self.canvas_boxes(cx) else {
            return Vec::new();
        };
        let Some(parent) = held
            .first()
            .and_then(|first| self.dom.parent(first.referent))
        else {
            return Vec::new();
        };
        let carried = |referent: Ref| held.iter().any(|h| h.referent == referent);
        let mut targets: Vec<Rect> = self
            .dom
            .get(parent)
            .map(|instance| instance.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|&child| !carried(child))
            .filter_map(|child| box_of(&boxes, child))
            .map(|placed| Rect::of(placed).turned_bounds(placed.rotation))
            .collect();
        let parent_box = match parent == screen.referent {
            true => Some(screen),
            false => box_of(&boxes, parent).copied(),
        };
        targets.extend(parent_box.map(|placed| Rect::of(&placed)));
        targets
    }

    /// One step of a drag in flight, to the pointer at `at`.
    pub(super) fn drag_to(
        &mut self,
        gesture: Gesture,
        at: [f32; 2],
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> Gesture {
        let view = self.ui.view;
        // Ctrl held lets go of every snap, Sketch's convention.
        let snapping = !modifiers.control;
        let reach = SNAP_REACH / view.zoom;
        match gesture {
            Gesture::Move {
                at: start,
                held,
                first,
                ..
            } => {
                let mut shift = [0, 1].map(|axis| (at[axis] - start[axis]) / view.zoom);
                let bounds = held
                    .iter()
                    .map(|h| h.rect.turned_bounds(h.rotation))
                    .reduce(|a, b| a.union(&b));
                self.ui.guides.clear();
                if let (Some(bounds), true) = (bounds, snapping) {
                    let targets = self.snap_targets(&held, cx);
                    let snap = guides::snap_move(&bounds.shifted(shift), &targets, reach);
                    shift = [0, 1].map(|axis| shift[axis] + snap[axis]);
                    self.ui.guides = guides::guides(&bounds.shifted(shift), &targets);
                }
                let writes: Vec<(Ref, &str, String)> = held
                    .iter()
                    .map(|h| {
                        let moved = position_shift(shift, h.parent_rotation(), h.anchor, [0.0; 2]);
                        (
                            h.referent,
                            "Position",
                            udim2_text(shifted(h.position, moved)),
                        )
                    })
                    .collect();
                self.write_drag(first, &writes, cx);
                Gesture::Move {
                    at: start,
                    held,
                    shift,
                    first: false,
                }
            }
            Gesture::Resize {
                at: start,
                held,
                handle,
                first,
                ..
            } => {
                let travel = [0, 1].map(|axis| (at[axis] - start[axis]) / view.zoom);
                let size = [held.rect.w, held.rect.h];
                let mut local = rotate(travel, -held.rotation);
                self.ui.guides.clear();
                // Snapping an edge is only meaningful while the box is
                // square to the screen; a turned one has no edge on a line.
                let targets = match snapping && held.rotation == 0.0 {
                    true => self.snap_targets(std::slice::from_ref(&held), cx),
                    false => Vec::new(),
                };
                let sides = [handle.0, handle.1];
                for axis in [0, 1] {
                    if sides[axis] == 0 || targets.is_empty() {
                        continue;
                    }
                    let step = resize(handle, size, local, false);
                    let (start_edge, length) = held.rect.along(axis);
                    let centre = start_edge + length * 0.5 + step.centre[axis];
                    let half = (length + step.grow[axis]) * 0.5;
                    let edge = centre + f32::from(sides[axis]) * half;
                    if let Some(snap) =
                        guides::snap_axis(&[edge], &guides::lines_of(&targets, axis), reach)
                    {
                        local[axis] += snap;
                    }
                }
                let step = resize(handle, size, local, modifiers.shift);
                let centre = rotate(step.centre, held.rotation);
                let rect = Rect {
                    x: held.rect.x + centre[0] - step.grow[0] * 0.5,
                    y: held.rect.y + centre[1] - step.grow[1] * 0.5,
                    w: held.rect.w + step.grow[0],
                    h: held.rect.h + step.grow[1],
                };
                if !targets.is_empty() {
                    self.ui.guides = guides::guides(&rect, &targets);
                }
                let moved = position_shift(centre, held.parent_rotation(), held.anchor, step.grow);
                let writes = [
                    (
                        held.referent,
                        "Size",
                        udim2_text(shifted(held.size, step.grow)),
                    ),
                    (
                        held.referent,
                        "Position",
                        udim2_text(shifted(held.position, moved)),
                    ),
                ];
                self.write_drag(first, &writes, cx);
                Gesture::Resize {
                    at: start,
                    held,
                    handle,
                    rect,
                    first: false,
                }
            }
            Gesture::Rotate {
                held, from, first, ..
            } => {
                let now = angle_of(held.rect.centre(), view.to_canvas(at));
                let mut rotation = held.own_rotation + (now - from);
                if modifiers.shift {
                    rotation = round_to(rotation, ROTATE_STEP);
                }
                let text = format!("{}", (rotation * 100.0).round() / 100.0);
                self.write_drag(first, &[(held.referent, "Rotation", text)], cx);
                Gesture::Rotate {
                    held,
                    from,
                    rotation,
                    first: false,
                }
            }
            Gesture::Marquee { from, extend, .. } => Gesture::Marquee {
                from,
                to: view.to_canvas(at),
                extend,
            },
            Gesture::Pan { last } => {
                self.ui.view.pan =
                    [0, 1].map(|axis| self.ui.view.pan[axis] + at[axis] - last[axis]);
                self.ui.fitted = false;
                Gesture::Pan { last: at }
            }
            pressed @ Gesture::Pressed { .. } => pressed,
        }
    }
}
