//! [`Held`]: an element as a gesture grabbed it, with every layout fact a
//! write back into its `UDim2`s has to honour — the box its parent lays it
//! out in (`UIPadding` taken off), its `UIScale`, whether an aspect
//! constraint shapes it, and its `SizeConstraint`. The first three are the
//! renderer's own answers (see `rbx_viewer::GuiBox`), never re-derived.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::GuiBox;

use super::super::is_gui_object;
use crate::ui_canvas::arrange::Member;
use crate::ui_canvas::carry::Carried;
use crate::ui_canvas::{box_of, rotate, Rect, Udim2};

/// The box an element's `UDim2`s resolve in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell::ui_editor) struct Parent {
    /// The parent's box less `UIPadding` — or the screen's own frame.
    pub(in crate::shell::ui_editor) content: Rect,
    /// The parent's own box: its children are placed square to `content`,
    /// then carried round this box's centre by `rotation`.
    pub(in crate::shell::ui_editor) rect: Rect,
    /// The parent's `AbsoluteRotation`.
    pub(in crate::shell::ui_editor) rotation: f32,
}

/// One element as a gesture grabbed it: its box as laid out, and the
/// properties a drag rewrites, as they stood.
#[derive(Debug, Clone, Copy)]
pub(in crate::shell::ui_editor) struct Held {
    pub(in crate::shell::ui_editor) referent: Ref,
    pub(in crate::shell::ui_editor) rect: Rect,
    /// `AbsoluteRotation`, and how much of it is the element's own
    /// `Rotation` rather than its ancestors'.
    pub(in crate::shell::ui_editor) rotation: f32,
    pub(in crate::shell::ui_editor) own_rotation: f32,
    pub(in crate::shell::ui_editor) anchor: [f32; 2],
    pub(in crate::shell::ui_editor) position: Udim2,
    pub(in crate::shell::ui_editor) size: Udim2,
    /// `UIScale`: a pixel of `Size` offset is this many on screen.
    pub(in crate::shell::ui_editor) size_scale: f32,
    /// An aspect constraint decides its shape, so a resize keeps it.
    pub(in crate::shell::ui_editor) aspect: bool,
    /// `SizeConstraint`: which parent axis each `Size` scale is taken
    /// against.
    pub(in crate::shell::ui_editor) size_axes: [usize; 2],
    /// `None` inside a `ScrollingFrame`, whose scrolled canvas the editor
    /// is not shown, and under a parent that is not laid out at all.
    pub(in crate::shell::ui_editor) parent: Option<Parent>,
}

impl Held {
    pub(in crate::shell::ui_editor) fn read(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        placed: &GuiBox,
        root: &GuiBox,
        boxes: &[GuiBox],
    ) -> Option<Held> {
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
            size_scale: placed.size_scale,
            aspect: placed.aspect,
            // `Enum.SizeConstraint`: RelativeXY, RelativeXX, RelativeYY.
            size_axes: match properties.get("SizeConstraint") {
                Some(Variant::Enum(1)) => [0, 0],
                Some(Variant::Enum(2)) => [1, 1],
                _ => [0, 1],
            },
            parent: parent_of(dom, database, placed.referent, root, boxes),
        })
    }

    /// What a `Position` scale of 1 comes to on each axis, in pixels: the
    /// parent's content box, or nothing where there is none to measure.
    pub(in crate::shell::ui_editor) fn position_span(&self) -> [f32; 2] {
        self.parent
            .map_or([0.0; 2], |parent| [parent.content.w, parent.content.h])
    }

    /// The same for `Size`, each axis measured along the parent axis
    /// `SizeConstraint` takes it from, before `UIScale`.
    pub(in crate::shell::ui_editor) fn size_span(&self) -> [f32; 2] {
        let span = self.position_span();
        self.size_axes.map(|axis| span[axis])
    }

    pub(in crate::shell::ui_editor) fn parent_rotation(&self) -> f32 {
        self.parent
            .map_or(self.rotation - self.own_rotation, |parent| parent.rotation)
    }

    /// Its box in its parent's own frame — turned back round the parent's
    /// centre, where [`Parent::content`] is square to the axes.
    pub(in crate::shell::ui_editor) fn local_rect(&self) -> Rect {
        let Some(parent) = self.parent.filter(|parent| parent.rotation != 0.0) else {
            return self.rect;
        };
        let pivot = parent.rect.centre();
        let centre = self.rect.centre();
        let [x, y] = rotate(
            [centre[0] - pivot[0], centre[1] - pivot[1]],
            -parent.rotation,
        );
        Rect {
            x: pivot[0] + x - self.rect.w * 0.5,
            y: pivot[1] + y - self.rect.h * 0.5,
            ..self.rect
        }
    }

    /// As a group takes it in — see `ui_canvas::arrange::group`.
    pub(in crate::shell::ui_editor) fn member(&self) -> Member {
        Member {
            rect: self.local_rect(),
            anchor: self.anchor,
            position: self.position,
            size: self.size,
            size_scale: self.size_scale,
            size_axes: self.size_axes,
        }
    }

    /// As a selection resize or turn carries it — see `ui_canvas::carry`.
    pub(in crate::shell::ui_editor) fn carried(&self) -> Carried {
        Carried {
            centre: self.rect.centre(),
            size: [self.rect.w, self.rect.h],
            rotation: self.rotation,
        }
    }
}

/// The box `referent`'s `UDim2`s resolve in: the nearest laid-out
/// `GuiObject` above it — a `Folder` between the two is a scope of its
/// own but lays its contents out in that same box — or the root's frame.
pub(in crate::shell::ui_editor) fn parent_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    root: &GuiBox,
    boxes: &[GuiBox],
) -> Option<Parent> {
    let mut up = dom.parent(referent);
    while let Some(parent) = up {
        let placed = match parent == root.referent {
            true => Some(root),
            false if is_gui_object(dom, database, parent) => box_of(boxes, parent),
            false => {
                up = dom.parent(parent);
                continue;
            }
        };
        let placed = placed?;
        return Some(Parent {
            content: Rect::from_array(placed.content?),
            rect: Rect::of(placed),
            rotation: placed.rotation,
        });
    }
    None
}
