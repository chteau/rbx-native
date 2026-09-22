//! Align, distribute and group for the canvas's selection. The geometry is
//! `ui_canvas::arrange`'s; this is where it meets the DOM — every result
//! is a `Position`/`Size` write through `Shell::write_drag`, or, for a
//! group, through the one push/record pair `shell::group` uses, so each is
//! one Ctrl+Z.

use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};

use super::super::Shell;
use super::gesture::Held;
use super::is_gui_object;
use crate::align::Mode;
use crate::properties;
use crate::ui_canvas::arrange::{self, Member};
use crate::ui_canvas::{box_of, position_shift, shifted, udim2_text, Rect};

impl Shell {
    /// Lines the selected elements' `mode` sides up on `axis`.
    pub(super) fn align_gui(&mut self, axis: usize, mode: Mode, cx: &mut Context<Self>) {
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let held = self.held_selection(&root, &boxes);
        let rects: Vec<(Ref, Rect)> = held
            .iter()
            .map(|h| (h.referent, h.rect.turned_bounds(h.rotation)))
            .collect();
        let shifts = arrange::align(&rects, axis, mode);
        self.shift_gui(
            &held,
            |referent| {
                shifts
                    .iter()
                    .find(|(r, _)| *r == referent)
                    .map(|&(_, shift)| shift)
            },
            cx,
        );
    }

    /// Spaces the selected elements evenly along `axis`.
    pub(super) fn distribute_gui(&mut self, axis: usize, cx: &mut Context<Self>) {
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let held = self.held_selection(&root, &boxes);
        let rects: Vec<Rect> = held
            .iter()
            .map(|h| h.rect.turned_bounds(h.rotation))
            .collect();
        let shifts = arrange::distribute(&rects, axis);
        self.shift_gui(
            &held,
            |referent| {
                let index = held.iter().position(|h| h.referent == referent)?;
                let mut shift = [0.0; 2];
                shift[axis] = shifts[index];
                Some(shift)
            },
            cx,
        );
    }

    /// Moves each of `held` by what `shift_of` says, on screen, as one step.
    fn shift_gui(
        &mut self,
        held: &[Held],
        shift_of: impl Fn(Ref) -> Option<[f32; 2]>,
        cx: &mut Context<Self>,
    ) {
        let writes: Vec<(Ref, &str, String)> = held
            .iter()
            .filter_map(|h| {
                let shift = shift_of(h.referent).filter(|shift| *shift != [0.0, 0.0])?;
                let moved = position_shift(shift, h.parent_rotation(), h.anchor, [0.0; 2]);
                Some((
                    h.referent,
                    "Position",
                    udim2_text(shifted(h.position, moved)),
                ))
            })
            .collect();
        if !writes.is_empty() {
            self.write_drag(true, &writes, cx);
        }
    }

    /// Ctrl+G on `GuiObject`s: wraps them in a transparent `Frame` fitted
    /// round them, where a `Model` would mean nothing to a GUI. `false` —
    /// leaving the ordinary group to go ahead — unless every one of
    /// `selected` is a `GuiObject` laid out on the canvas.
    pub(in crate::shell) fn group_gui(
        &mut self,
        selected: &[Ref],
        parent: Option<Ref>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(parent) = parent else {
            return false;
        };
        if !selected
            .iter()
            .all(|&r| is_gui_object(&self.dom, &self.database, r))
        {
            return false;
        }
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return false;
        };
        let held: Option<Vec<Held>> = selected
            .iter()
            .map(|&r| {
                let placed = box_of(&boxes, r)?;
                Held::read(&self.dom, &self.database, placed, &root, &boxes)
            })
            .collect();
        let Some(held) = held else {
            return false;
        };
        // Every member resolves in the same box — they share a parent — and
        // `None` there means a `ScrollingFrame`'s canvas, which the group
        // then measures from its first member (see `arrange::group`).
        let content = held
            .first()
            .and_then(|first| first.parent)
            .map(|parent| parent.content);
        let members: Vec<Member> = held.iter().map(Held::member).collect();
        let Some(grouping) = arrange::group(&members, content) else {
            return false;
        };

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let frame = dom.new_instance("Frame", "Group", Some(parent));
        let (position, size) = grouping.frame;
        let mut writes = vec![
            (frame, "Position", udim2_text(position)),
            (frame, "Size", udim2_text(size)),
            (frame, "BackgroundTransparency", "1".to_owned()),
            (frame, "BorderSizePixel", "0".to_owned()),
        ];
        for (&member, (position, size)) in selected.iter().zip(grouping.members) {
            dom.set_parent(member, Some(frame));
            writes.push((member, "Position", udim2_text(position)));
            writes.push((member, "Size", udim2_text(size)));
        }
        let written = writes.iter().try_for_each(|(referent, name, text)| {
            properties::edit::commit(&mut dom, &self.database, *referent, name, text).map(|_| ())
        });
        self.dom = dom;
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reselect(vec![frame], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Err(err) = written {
            self.output.push_warning(&format!("group: {err}"));
        }
        cx.notify();
        true
    }
}
