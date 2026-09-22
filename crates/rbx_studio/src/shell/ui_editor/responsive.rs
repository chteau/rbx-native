//! "Make responsive": the selection — or, with none on the canvas, the
//! whole screen — rewritten so it scales with the screen instead of
//! staying the same number of pixels on every device.
//!
//! Every `Position` and `Size` offset is folded into its scale against the
//! parent's content as laid out right now (`ui_canvas::arrange::to_scale`),
//! so nothing moves at the resolution on the canvas; and every element that
//! was pixels alone on both axes gets a `UIAspectRatioConstraint` at the
//! shape it has, so a square button stays a square on a phone. One undo
//! step for the lot, like any other edit.

use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_viewer::GuiBox;

use super::super::Shell;
use super::gesture::Held;
use super::is_gui_object;
use crate::properties;
use crate::ui_canvas::arrange::{is_fixed, to_scale};
use crate::ui_canvas::{box_of, udim2_text};

const SCROLLING_CLASS: &str = "ScrollingFrame";
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

        // Every laid-out `GuiObject` at or under the roots, each with the
        // extent its parent lays it out in.
        let mut targets: Vec<(Held, [f32; 2])> = Vec::new();
        let mut pending = roots;
        while let Some(referent) = pending.pop() {
            if let Some(instance) = self.dom.get(referent) {
                pending.extend(instance.children().iter().copied());
            }
            if targets.iter().any(|(held, _)| held.referent == referent) {
                continue;
            }
            let held = box_of(&boxes, referent).and_then(|placed| Held::read(&self.dom, placed));
            let extent = self.content_extent(referent, &screen, &boxes);
            if let (Some(held), Some(extent)) = (held, extent) {
                targets.push((held, extent));
            }
        }
        if targets.is_empty() {
            return;
        }

        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let mut written = Ok(());
        for (held, extent) in &targets {
            let mut writes = vec![
                (
                    held.referent,
                    "Position",
                    udim2_text(to_scale(held.position, *extent)),
                ),
                (
                    held.referent,
                    "Size",
                    udim2_text(to_scale(held.size, *extent)),
                ),
            ];
            let shaped = dom.get(held.referent).is_some_and(|instance| {
                instance.children().iter().any(|&child| {
                    dom.get(child)
                        .is_some_and(|c| self.database.is_subclass_of(c.class(), ASPECT_CLASS))
                })
            });
            if is_fixed(held.size) && !shaped && held.rect.h > 0.0 {
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

    /// The pixels `referent`'s `UDim2`s resolve against: its nearest
    /// `GuiObject` ancestor's box, or the screen's own frame — `None` inside
    /// a `ScrollingFrame`, whose children resolve against its canvas rather
    /// than the window the box is. A `UIPadding` on the parent is not taken
    /// off; the canvas lays out boxes, not the content inside them.
    fn content_extent(&self, referent: Ref, screen: &GuiBox, boxes: &[GuiBox]) -> Option<[f32; 2]> {
        let mut up = self.dom.parent(referent);
        while let Some(parent) = up {
            if parent == screen.referent {
                return Some([screen.rect[2], screen.rect[3]]);
            }
            if is_gui_object(&self.dom, &self.database, parent) {
                let class = self.dom.get(parent)?.class();
                if self.database.is_subclass_of(class, SCROLLING_CLASS) {
                    return None;
                }
                let placed = box_of(boxes, parent)?;
                return Some([placed.rect[2], placed.rect[3]]);
            }
            up = self.dom.parent(parent);
        }
        None
    }
}
