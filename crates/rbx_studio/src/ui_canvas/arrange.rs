//! Align, distribute and group, for boxes on the canvas.
//!
//! Aligning is the Align tool's own geometry (`crate::align`), fed a flat
//! box per element on the canvas's two axes: Min/Center/Max against the
//! selection's bounds is the same question in 2D as in 3D, and one answer
//! for both keeps the two tools from ever disagreeing about it.

use glam::{Mat4, Vec3};
use rbx_dom::Ref;

use super::{Rect, Udim2};
use crate::align::{self, Mode, Options, RelativeTo, Space};
use crate::transform::Target;

/// How far each box moves to line its `mode` side up with the selection's
/// on `axis` (0 across, 1 down) — Min is left/top, Max right/bottom. Boxes
/// already there are left out. Fewer than two boxes align nothing.
pub(crate) fn align(boxes: &[(Ref, Rect)], axis: usize, mode: Mode) -> Vec<(Ref, [f32; 2])> {
    let entries: Vec<Vec<Target>> = boxes
        .iter()
        .map(|&(referent, rect)| {
            let [x, y] = rect.centre();
            vec![Target {
                referent,
                // A unit-deep slab: `align` measures a box by its axes'
                // lengths, and a zero one would have no orientation.
                model: Mat4::from_translation(Vec3::new(x, y, 0.0))
                    * Mat4::from_scale(Vec3::new(rect.w, rect.h, 1.0)),
                sphere: false,
                cylinder: false,
            }]
        })
        .collect();
    let mut options = Options::default();
    options.mode = mode;
    options.space = Space::World;
    options.relative_to = RelativeTo::SelectionBounds;
    options.set_axes([axis == 0, axis == 1, false]);
    align::plan(&entries, 0, options)
        .into_iter()
        .filter_map(|(referent, position)| {
            let (_, rect) = boxes.iter().find(|(r, _)| *r == referent)?;
            let [x, y] = rect.centre();
            let shift = [position.x - x, position.y - y];
            (shift != [0.0, 0.0]).then_some((referent, shift))
        })
        .collect()
}

/// How far each box moves along `axis` for the gaps between them to come
/// out equal, the first and last staying where they are — index for index
/// with `boxes`. Fewer than three boxes have nothing between them to space.
pub(crate) fn distribute(boxes: &[Rect], axis: usize) -> Vec<f32> {
    let mut shifts = vec![0.0; boxes.len()];
    if boxes.len() < 3 {
        return shifts;
    }
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| boxes[a].along(axis).0.total_cmp(&boxes[b].along(axis).0));
    let (start, _) = boxes[order[0]].along(axis);
    let end = order
        .iter()
        .map(|&index| {
            let (from, length) = boxes[index].along(axis);
            from + length
        })
        .fold(f32::MIN, f32::max);
    let taken: f32 = boxes.iter().map(|rect| rect.along(axis).1).sum();
    let gap = (end - start - taken) / (boxes.len() - 1) as f32;
    let mut cursor = start;
    for index in order {
        let (from, length) = boxes[index].along(axis);
        shifts[index] = cursor - from;
        cursor += length + gap;
    }
    shifts
}

/// `udim` with its offsets folded into its scales against a parent whose
/// content is `parent` pixels across: the very same pixels at this size,
/// and a box that grows and shrinks with its parent at any other — what
/// "responsive" means for a `UDim2`. An axis with no parent extent to
/// divide by is left as it was.
pub(crate) fn to_scale(udim: Udim2, parent: [f32; 2]) -> Udim2 {
    [0, 1].map(|axis| {
        let (scale, offset) = udim[axis];
        match parent[axis] > 0.0 {
            true => (scale + offset as f32 / parent[axis], 0),
            false => (scale, offset),
        }
    })
}

/// Whether a `Size` is pixels alone on both axes — a box drawn to a fixed
/// shape, which a responsive pass keeps that shape with an aspect ratio.
pub(crate) fn is_fixed(size: Udim2) -> bool {
    size.iter().all(|&(scale, _)| scale == 0.0)
}

/// One element a group takes in: where it came out, its `AnchorPoint`, and
/// the `Position` it came out there from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Member {
    pub(crate) rect: Rect,
    pub(crate) anchor: [f32; 2],
    pub(crate) position: Udim2,
}

/// What wrapping members in a new frame writes: the frame's own `Position`
/// and `Size` in its parent, and each member's new `Position` and `Size`
/// inside the frame, in the members' order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grouping {
    pub(crate) frame: (Udim2, Udim2),
    pub(crate) members: Vec<(Udim2, Udim2)>,
}

/// A frame fitted exactly round `members`, which all share one parent whose
/// content area is `parent_size` pixels, with each member moved inside it
/// so that nothing on screen moves.
///
/// Everything comes out in offsets — the frame's box and each member's
/// place in it are measured in pixels at the resolution on the canvas, and
/// a scale would stretch a member against a frame it was never sized for.
/// The parent's origin is recovered from the first member: its box is where
/// its `Position` put its `AnchorPoint`.
pub(crate) fn group(members: &[Member], parent_size: [f32; 2]) -> Option<Grouping> {
    let first = members.first()?;
    let origin = [0, 1].map(|axis| {
        let (start, length) = first.rect.along(axis);
        let (scale, offset) = first.position[axis];
        start + first.anchor[axis] * length - scale * parent_size[axis] - offset as f32
    });
    let bounds = members
        .iter()
        .skip(1)
        .fold(first.rect, |bounds, member| bounds.union(&member.rect));
    let offsets = |values: [f32; 2]| values.map(|value| (0.0, value.round() as i32));
    Some(Grouping {
        frame: (
            offsets([bounds.x - origin[0], bounds.y - origin[1]]),
            offsets([bounds.w, bounds.h]),
        ),
        members: members
            .iter()
            .map(|member| {
                let rect = member.rect;
                (
                    offsets([
                        rect.x + member.anchor[0] * rect.w - bounds.x,
                        rect.y + member.anchor[1] * rect.h - bounds.y,
                    ]),
                    offsets([rect.w, rect.h]),
                )
            })
            .collect(),
    })
}
