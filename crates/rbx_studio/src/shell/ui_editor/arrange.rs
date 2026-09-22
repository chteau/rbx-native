//! Align, distribute and group for the canvas's selection. The geometry is
//! `ui_canvas::arrange`'s; this is where it meets the DOM — every result
//! is a `Position`/`Size` write through `Shell::write_drag`, or, for a
//! group, through the one push/record pair `shell::group` uses, so each is
//! one Ctrl+Z.

use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::super::Shell;
use super::gesture::{held, Held};
use super::is_gui_object;
use super::tree::Writes;
use crate::align::Mode;
use crate::ui_canvas::arrange::{self, Member};
use crate::ui_canvas::{box_of, position_shift, rotate, shifted_in, udim2_text, Rect};

impl Shell {
    /// Lines the selected elements' `mode` sides up on `axis` — or one
    /// element's with its parent's content box, as Figma's Position buttons
    /// do with a lone layer.
    pub(super) fn align_gui(&mut self, axis: usize, mode: Mode, cx: &mut Context<Self>) {
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let held = self.held_selection(&root, &boxes);
        if let [one] = held.as_slice() {
            let Some(parent) = one.parent else {
                return;
            };
            let (from, length) = one.local_rect().turned_bounds(one.own_rotation).along(axis);
            let (start, extent) = parent.content.along(axis);
            let mut local = [0.0; 2];
            local[axis] = match mode {
                Mode::Min => start - from,
                Mode::Center => start + (extent - length) * 0.5 - from,
                Mode::Max => start + extent - length - from,
            };
            let shift = rotate(local, parent.rotation);
            self.shift_gui(&held, |_| Some(shift), cx);
            return;
        }
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
                let position = shifted_in(h.position, moved, self.ui.unit, h.position_span());
                Some((h.referent, "Position", udim2_text(position)))
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

        let (position, size) = grouping.frame;
        let selected = selected.to_vec();
        self.edit_gui_tree(
            "group",
            |dom, _| {
                let frame = dom.new_instance("Frame", "Group", Some(parent));
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
                (writes, Some(vec![frame]))
            },
            cx,
        );
        true
    }

    /// Whether every selected instance is a `Frame` Ctrl+Shift+G would take
    /// apart on the canvas.
    pub(in crate::shell) fn ui_can_ungroup(&self) -> bool {
        let selected = self.selected_all();
        !selected.is_empty()
            && selected.iter().all(|&r| {
                self.dom.get(r).is_some_and(|i| i.class() == "Frame")
                    && self.dom.parent(r).is_some()
            })
    }

    /// Ctrl+Shift+G on `Frame`s on the canvas: each one's elements move up
    /// into its parent where they stand on screen, each value keeping its
    /// mode, and the frame goes — with the modifiers that styled it, as a
    /// Figma group's own fill goes. `false`, leaving the ordinary ungroup
    /// to go ahead, unless every one of `selected` is such a frame.
    pub(in crate::shell) fn ungroup_gui(
        &mut self,
        selected: &[Ref],
        cx: &mut Context<Self>,
    ) -> bool {
        let frames = selected.iter().all(|&r| {
            self.dom
                .get(r)
                .is_some_and(|instance| instance.class() == "Frame")
        });
        let Some((root, boxes)) = self.canvas_boxes(cx).filter(|_| frames) else {
            return false;
        };
        let mut plan: Vec<(Ref, Ref, Writes)> = Vec::new();
        for &frame in selected {
            let Some(grand) = held::parent_of(&self.dom, &self.database, frame, &root, &boxes)
            else {
                return false;
            };
            let (Some(up), Some(instance)) = (self.dom.parent(frame), self.dom.get(frame)) else {
                return false;
            };
            let turn = match instance.properties().get("Rotation") {
                Some(Variant::Float32(degrees)) => *degrees,
                _ => 0.0,
            };
            let corner = [grand.content.x, grand.content.y];
            let extent = [grand.content.w, grand.content.h];
            let mut writes = Writes::new();
            for &child in instance.children() {
                let Some(mut h) = box_of(&boxes, child).and_then(|placed| {
                    Held::read(&self.dom, &self.database, placed, &root, &boxes)
                }) else {
                    continue;
                };
                h.parent = Some(grand);
                let member = h.member();
                let (position, size) = arrange::place(&member, corner, extent, member.scaled());
                writes.push((child, "Position", udim2_text(position)));
                writes.push((child, "Size", udim2_text(size)));
                if turn != 0.0 {
                    writes.push((child, "Rotation", format!("{}", h.own_rotation + turn)));
                }
            }
            plan.push((frame, up, writes));
        }
        self.edit_gui_tree(
            "ungroup",
            |dom, _| {
                let mut freed = Vec::new();
                let mut all = Writes::new();
                for (frame, up, writes) in plan {
                    for (child, ..) in &writes {
                        if !freed.contains(child) {
                            dom.set_parent(*child, Some(up));
                            freed.push(*child);
                        }
                    }
                    dom.remove(frame);
                    all.extend(writes);
                }
                (all, Some(freed))
            },
            cx,
        );
        true
    }
}
