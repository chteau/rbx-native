//! One step of a drag in flight: where it moves what it holds, what it
//! snaps onto on the way, and the writes that follow.

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::snap::round_to;
use rbx_viewer::GuiBox;

use super::super::super::Shell;
use super::super::is_gui_object;
use super::{Gesture, Held, ROTATE_STEP, SNAP_REACH};
use crate::ui_canvas::carry::{self, Carried};
use crate::ui_canvas::guides;
use crate::ui_canvas::{
    angle_of, box_of, centred, position_shift, resize, rotate, shifted_in, udim2_text, Rect,
};

impl Shell {
    /// Every selected element a move carries: the laid-out `GuiObject`s,
    /// less any whose ancestor is carried too — moving the parent already
    /// moves it.
    pub(in crate::shell::ui_editor) fn held_selection(
        &self,
        root: &GuiBox,
        boxes: &[GuiBox],
    ) -> Vec<Held> {
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
            .filter_map(|placed| Held::read(&self.dom, &self.database, placed, root, boxes))
            .collect()
    }

    /// The boxes a drag of `held` snaps onto: its siblings, and its
    /// parent's box and the padded box inside it — the screen's frame when
    /// the parent is the screen.
    fn snap_targets(&self, held: &[Held], cx: &App) -> Vec<Rect> {
        let Some((_, boxes)) = self.canvas_boxes(cx) else {
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
        if let Some(frame) = held.first().and_then(|first| first.parent) {
            targets.push(frame.rect);
            if frame.content != frame.rect {
                targets.push(frame.content);
            }
        }
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
        let unit = self.ui.unit;
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
                // Shift holds the move to the axis it has gone further along.
                let locked = modifiers
                    .shift
                    .then(|| usize::from(shift[0].abs() >= shift[1].abs()));
                if let Some(axis) = locked {
                    shift[axis] = 0.0;
                }
                let bounds = held
                    .iter()
                    .map(|h| h.rect.turned_bounds(h.rotation))
                    .reduce(|a, b| a.union(&b));
                self.ui.guides.clear();
                if let (Some(bounds), true) = (bounds, snapping) {
                    let targets = self.snap_targets(&held, cx);
                    let snap = guides::snap_move(&bounds.shifted(shift), &targets, reach);
                    shift = [0, 1].map(|axis| match Some(axis) == locked {
                        true => 0.0,
                        false => shift[axis] + snap[axis],
                    });
                    self.ui.guides = guides::guides(&bounds.shifted(shift), &targets);
                }
                let writes: Vec<(Ref, &str, String)> = held
                    .iter()
                    .map(|h| {
                        let moved = position_shift(shift, h.parent_rotation(), h.anchor, [0.0; 2]);
                        let position = shifted_in(h.position, moved, unit, h.position_span());
                        (h.referent, "Position", udim2_text(position))
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
                frame: (grab, turn),
                first,
                ..
            } => {
                let travel = [0, 1].map(|axis| (at[axis] - start[axis]) / view.zoom);
                let size = [grab.w, grab.h];
                let mut local = rotate(travel, -turn);
                self.ui.guides.clear();
                // Snapping an edge is only meaningful while the frame is
                // square to the screen; a turned one has no edge on a line.
                // Alt resizes about the centre, where one edge's snap would
                // throw the other off it.
                let targets = match snapping && turn == 0.0 && !modifiers.alt {
                    true => self.snap_targets(&held, cx),
                    false => Vec::new(),
                };
                let sides = [handle.0, handle.1];
                for axis in [0, 1] {
                    if sides[axis] == 0 || targets.is_empty() {
                        continue;
                    }
                    let step = resize(handle, size, local, false);
                    let (start_edge, length) = grab.along(axis);
                    let centre = start_edge + length * 0.5 + step.centre[axis];
                    let half = (length + step.grow[axis]) * 0.5;
                    let edge = centre + f32::from(sides[axis]) * half;
                    if let Some(snap) =
                        guides::snap_axis(&[edge], &guides::lines_of(&targets, axis), reach)
                    {
                        local[axis] += snap;
                    }
                }
                // One element whose shape an aspect constraint decides keeps
                // its shape: a free stretch would only snap back once drawn.
                let keep = modifiers.shift || matches!(held.as_slice(), [one] if one.aspect);
                let mut step = resize(handle, size, local, keep);
                if modifiers.alt {
                    step = centred(step, size);
                }
                let centre = rotate(step.centre, turn);
                let shown = Rect {
                    x: grab.x + centre[0] - step.grow[0] * 0.5,
                    y: grab.y + centre[1] - step.grow[1] * 0.5,
                    w: grab.w + step.grow[0],
                    h: grab.h + step.grow[1],
                };
                if !targets.is_empty() {
                    self.ui.guides = guides::guides(&shown, &targets);
                }
                let carried: Vec<Carried> = held.iter().map(Held::carried).collect();
                let moved = carry::scale(&grab, turn, &step, &carried);
                let mut writes: Vec<(Ref, &str, String)> = Vec::with_capacity(held.len() * 2);
                let mut preview = Vec::with_capacity(held.len());
                for (h, m) in held.iter().zip(&moved) {
                    // `UIScale` multiplies the whole resolved `Size`, so a
                    // pixel on screen is less than a pixel of offset.
                    let offsets = m.grow.map(|grow| grow / h.size_scale);
                    let position = position_shift(m.centre, h.parent_rotation(), h.anchor, m.grow);
                    let size = shifted_in(h.size, offsets, unit, h.size_span());
                    let position = shifted_in(h.position, position, unit, h.position_span());
                    writes.push((h.referent, "Size", udim2_text(size)));
                    writes.push((h.referent, "Position", udim2_text(position)));
                    let rect = Rect {
                        x: h.rect.x + m.centre[0] - m.grow[0] * 0.5,
                        y: h.rect.y + m.centre[1] - m.grow[1] * 0.5,
                        w: h.rect.w + m.grow[0],
                        h: h.rect.h + m.grow[1],
                    };
                    preview.push((h.referent, rect, h.rotation));
                }
                self.write_drag(first, &writes, cx);
                Gesture::Resize {
                    at: start,
                    held,
                    handle,
                    frame: (grab, turn),
                    shown: (shown, turn),
                    preview,
                    first: false,
                }
            }
            Gesture::Rotate {
                held,
                frame,
                from,
                first,
                ..
            } => {
                let pivot = frame.0.centre();
                let mut delta = angle_of(pivot, view.to_canvas(at)) - from;
                if modifiers.shift {
                    // One element lands on a round angle; a selection turns
                    // by one, since its members need not share an angle.
                    delta = match held.as_slice() {
                        [one] => round_to(one.own_rotation + delta, ROTATE_STEP) - one.own_rotation,
                        _ => round_to(delta, ROTATE_STEP),
                    };
                }
                let carried: Vec<Carried> = held.iter().map(Held::carried).collect();
                let moved = carry::turn(pivot, delta, &carried);
                let mut writes: Vec<(Ref, &str, String)> = Vec::with_capacity(held.len() * 2);
                for (h, shift) in held.iter().zip(moved) {
                    let rotation = ((h.own_rotation + delta) * 100.0).round() / 100.0;
                    writes.push((h.referent, "Rotation", format!("{rotation}")));
                    if shift != [0.0, 0.0] {
                        let moved = position_shift(shift, h.parent_rotation(), h.anchor, [0.0; 2]);
                        let position = shifted_in(h.position, moved, unit, h.position_span());
                        writes.push((h.referent, "Position", udim2_text(position)));
                    }
                }
                self.write_drag(first, &writes, cx);
                Gesture::Rotate {
                    held,
                    frame,
                    from,
                    delta,
                    first: false,
                }
            }
            Gesture::Marquee { from, extend, .. } => Gesture::Marquee {
                from,
                to: view.to_canvas(at),
                extend,
            },
            Gesture::Draw { class, from, .. } => {
                let mut to = view.to_canvas(at);
                // Shift draws a square, on the longer of the two sides.
                if modifiers.shift {
                    let side = (to[0] - from[0]).abs().max((to[1] - from[1]).abs());
                    to = [0, 1].map(|axis| from[axis] + side.copysign(to[axis] - from[axis]));
                }
                Gesture::Draw { class, from, to }
            }
            Gesture::Radius {
                corner,
                frame,
                start,
                grab,
            } => {
                let reach = super::radius_at(&frame.0, frame.1, corner, view.to_canvas(at));
                let most = frame.0.w.min(frame.0.h) * 0.5;
                let radius = (start + reach - grab).clamp(0.0, most.max(start));
                self.drag_value_to(radius.round(), cx);
                Gesture::Radius {
                    corner,
                    frame,
                    start,
                    grab,
                }
            }
            Gesture::Band {
                band,
                at: from,
                start,
            } => {
                let travel = (at[band.axis] - from[band.axis]) / view.zoom * band.sign;
                self.drag_value_to((start + travel).round().max(0.0), cx);
                Gesture::Band {
                    band,
                    at: from,
                    start,
                }
            }
            Gesture::Reorder {
                referent,
                layout,
                others,
                grid,
                axis,
                ..
            } => Gesture::Reorder {
                referent,
                layout,
                others,
                grid,
                axis,
                to: view.to_canvas(at),
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
