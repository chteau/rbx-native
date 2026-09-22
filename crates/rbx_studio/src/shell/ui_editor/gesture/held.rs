//! [`Held`]: an element as a gesture grabbed it.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::GuiBox;

use crate::ui_canvas::arrange::Member;
use crate::ui_canvas::{Rect, Udim2};

/// One element as a gesture grabbed it: its box as laid out, and the
/// properties a drag rewrites, as they stood.
#[derive(Debug, Clone, Copy)]
pub(in crate::shell::ui_editor) struct Held {
    pub(in crate::shell::ui_editor) referent: Ref,
    pub(in crate::shell::ui_editor) rect: Rect,
    /// `AbsoluteRotation`, and how much of it is the element's own
    /// `Rotation` rather than its ancestors'.
    pub(in crate::shell::ui_editor) rotation: f32,
    pub(super) own_rotation: f32,
    pub(in crate::shell::ui_editor) anchor: [f32; 2],
    pub(in crate::shell::ui_editor) position: Udim2,
    pub(super) size: Udim2,
}

impl Held {
    pub(super) fn read(dom: &WeakDom, placed: &GuiBox) -> Option<Held> {
        let properties = dom.get(placed.referent)?.properties();
        let udim2 = |name: &str| match properties.get(name) {
            Some(Variant::UDim2(value)) => [
                (value.x.scale, value.x.offset),
                (value.y.scale, value.y.offset),
            ],
            _ => [(0.0, 0); 2],
        };
        Some(Held {
            referent: placed.referent,
            rect: Rect::of(placed),
            rotation: placed.rotation,
            own_rotation: match properties.get("Rotation") {
                Some(Variant::Float32(degrees)) => *degrees,
                _ => 0.0,
            },
            anchor: match properties.get("AnchorPoint") {
                Some(Variant::Vector2(anchor)) => [anchor.x, anchor.y],
                _ => [0.0, 0.0],
            },
            position: udim2("Position"),
            size: udim2("Size"),
        })
    }

    pub(in crate::shell::ui_editor) fn parent_rotation(&self) -> f32 {
        self.rotation - self.own_rotation
    }

    /// The same read, as a group takes a member in.
    pub(in crate::shell::ui_editor) fn read_member(
        dom: &WeakDom,
        placed: &GuiBox,
    ) -> Option<Member> {
        let held = Held::read(dom, placed)?;
        Some(Member {
            rect: held.rect,
            anchor: held.anchor,
            position: held.position,
        })
    }
}
