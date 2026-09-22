//! "Make responsive": the selection — or, with none on the canvas, the
//! whole screen — rewritten so it scales with the screen instead of
//! staying the same number of pixels on every device.
//!
//! Every `Position` and `Size` offset is folded into its scale against the
//! parent's content box as laid out right now — `UIPadding` taken off, a
//! `Size` measured along the axes its `SizeConstraint` names; `UIScale`
//! multiplies both halves alike and so cancels — using
//! `ui_canvas::arrange::to_scale`, so nothing moves at the resolution on the
//! canvas; and every element that was pixels alone on both axes, and is not
//! already shaped by one, gets a `UIAspectRatioConstraint` at the shape it
//! has, so a square button stays a square on a phone. One undo
//! step for the lot, like any other edit.

use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};

use super::super::Shell;
use super::gesture::Held;
use crate::properties;
use crate::ui_canvas::arrange::{is_fixed, to_scale};
use crate::ui_canvas::{box_of, udim2_text, Rect};

const ASPECT_CLASS: &str = "UIAspectRatioConstraint";

impl Shell {
    pub(super) fn make_responsive(&mut self, cx: &mut Context<Self>) {
        let Some((screen, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let mut roots: Vec<Ref> = self
            .selected_all()
            .iter()
            .copied()
            .filter(|&r| r == screen.referent || box_of(&boxes, r).is_some())
            .collect();
        if roots.is_empty() {
            roots.push(screen.referent);
        }

        // Every laid-out `GuiObject` at or under the roots whose parent's
        // box is known — not one inside a `ScrollingFrame`, which resolves
        // against a canvas the editor is not shown.
        let mut targets: Vec<(Held, Rect)> = Vec::new();
        let mut pending = roots;
        while let Some(referent) = pending.pop() {
            if let Some(instance) = self.dom.get(referent) {
                pending.extend(instance.children().iter().copied());
            }
            if targets.iter().any(|(held, _)| held.referent == referent) {
                continue;
            }
            let held = box_of(&boxes, referent)
                .and_then(|placed| Held::read(&self.dom, &self.database, placed, &screen, &boxes));
            if let Some((held, parent)) = held.and_then(|held| Some((held, held.parent?))) {
                targets.push((held, parent.content));
            }
        }
        if targets.is_empty() {
            return;
        }

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let mut written = Ok(());
        for (held, content) in &targets {
            // Against the parent's padded box: a `Position` along both of
            // its axes, a `Size` along whichever `SizeConstraint` names.
            let extent = [content.w, content.h];
            let size_extent = held.size_axes.map(|axis| extent[axis]);
            let mut writes = vec![
                (
                    held.referent,
                    "Position",
                    udim2_text(to_scale(held.position, extent)),
                ),
                (
                    held.referent,
                    "Size",
                    udim2_text(to_scale(held.size, size_extent)),
                ),
            ];
            if is_fixed(held.size) && !held.aspect && held.rect.h > 0.0 {
                let constraint = dom.new_instance(ASPECT_CLASS, ASPECT_CLASS, Some(held.referent));
                writes.push((
                    constraint,
                    "AspectRatio",
                    (held.rect.w / held.rect.h).to_string(),
                ));
            }
            for (referent, name, text) in writes {
                let result =
                    properties::edit::commit(&mut dom, &self.database, referent, name, &text);
                if let Err(err) = result {
                    written = Err(err);
                }
            }
        }
        self.dom = dom;
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        let kept = self.selected_all().to_vec();
        self.reselect(kept, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Err(err) = written {
            self.output.push_warning(&format!("make responsive: {err}"));
        }
        cx.notify();
    }
}
