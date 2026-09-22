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
/// the `Position`/`Size` it came out there from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Member {
    /// Its box in its parent's own frame — turned back by the parent's
    /// rotation, where the parent's content box is square to the axes.
    pub(crate) rect: Rect,
    pub(crate) anchor: [f32; 2],
    pub(crate) position: Udim2,
    pub(crate) size: Udim2,
    /// `UIScale`: its resolved `Size` times this is the box drawn.
    pub(crate) size_scale: f32,
    /// Which of the parent's axes each `Size` scale is taken against —
    /// `SizeConstraint`, `[0, 1]` unless it says otherwise.
    pub(crate) size_axes: [usize; 2],
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
/// children resolve against `parent` (its content box: less `UIPadding`),
/// with each member moved inside it so that nothing on screen moves.
///
/// Each value keeps the mode it had: a member placed or sized by scale on
/// an axis is written as a scale of the new frame there, one in pixels as
/// pixels, and the frame itself scales with its parent on any axis a member
/// did — so a responsive layout stays responsive once grouped. `UIScale` is
/// divided back out of a member's size, and `SizeConstraint` decides which
/// of the frame's axes a size scale is taken against.
///
/// `parent` is `None` inside a `ScrollingFrame`, whose scrolled canvas the
/// editor is not shown: the origin is then recovered from the first member
/// as though its `Position` were pixels alone, and the frame is written in
/// pixels.
pub(crate) fn group(members: &[Member], parent: Option<Rect>) -> Option<Grouping> {
    let first = members.first()?;
    let (origin, extent) = match parent {
        Some(content) => ([content.x, content.y], Some([content.w, content.h])),
        None => (
            [0, 1].map(|axis| {
                let (start, length) = first.rect.along(axis);
                start + first.anchor[axis] * length - first.position[axis].1 as f32
            }),
            None,
        ),
    };
    let bounds = members
        .iter()
        .skip(1)
        .fold(first.rect, |bounds, member| bounds.union(&member.rect));
    let size = [bounds.w, bounds.h];
    let scaled = [0, 1].map(|axis| {
        members
            .iter()
            .any(|member| member.position[axis].0 != 0.0 || member.size[axis].0 != 0.0)
    });
    let corner = [bounds.x, bounds.y];
    Some(Grouping {
        frame: (
            [0, 1].map(|axis| {
                let against = extent.map(|extent| extent[axis]);
                udim(corner[axis] - origin[axis], scaled[axis], against)
            }),
            [0, 1].map(|axis| udim(size[axis], scaled[axis], extent.map(|extent| extent[axis]))),
        ),
        members: members
            .iter()
            .map(|member| {
                let rect = member.rect;
                let placed = [
                    rect.x + member.anchor[0] * rect.w,
                    rect.y + member.anchor[1] * rect.h,
                ];
                let drawn = [rect.w, rect.h].map(|length| length / member.size_scale);
                (
                    [0, 1].map(|axis| {
                        let scaled = member.position[axis].0 != 0.0;
                        udim(placed[axis] - corner[axis], scaled, Some(size[axis]))
                    }),
                    [0, 1].map(|axis| {
                        let scaled = member.size[axis].0 != 0.0;
                        udim(drawn[axis], scaled, Some(size[member.size_axes[axis]]))
                    }),
                )
            })
            .collect(),
    })
}

/// `pixels` as a `UDim`: a scale of `against` when `scaled` and there is an
/// extent to divide by, whole pixels otherwise.
fn udim(pixels: f32, scaled: bool, against: Option<f32>) -> (f32, i32) {
    match (scaled, against) {
        (true, Some(extent)) if extent > 0.0 => (pixels / extent, 0),
        _ => (0.0, pixels.round() as i32),
    }
}
