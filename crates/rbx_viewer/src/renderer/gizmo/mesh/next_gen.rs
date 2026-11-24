//! What the "Next-Gen Draggers" skin draws on top of the classic pieces
//! (see `crate::gizmo::Skin`): Move's plane squares, Rotate's view ring, and
//! the skin's axis colours.

use glam::Vec3;

use super::{push, tube_slice, Vertex, RING_SEGMENTS};
use crate::gizmo::{Axis, Handles, Skin, PLANE_HALF, RING_THICKNESS, VIEW_COLOR, VIEW_RING_RADIUS};

/// Move's plane squares, the `shown` ones (all but the one a drag holds go
/// while it does, as do all of them under an arrow drag), furthest first,
/// each drawn from both sides (a flat square has no outside to cull by).
pub(in crate::renderer::gizmo) fn planes(
    handles: &Handles,
    eye: Vec3,
    shown: impl Fn(Axis) -> bool,
) -> Vec<Vertex> {
    let mut order: Vec<(f32, Axis)> = Axis::ALL
        .into_iter()
        .filter(|&axis| shown(axis))
        .map(|axis| ((handles.plane(axis).2 - eye).length(), axis))
        .collect();
    order.sort_by(|(a, _), (b, _)| b.total_cmp(a));

    let half = PLANE_HALF * handles.arm();
    let mut vertices = Vec::with_capacity(Axis::ALL.len() * 12);
    for (_, axis) in order {
        let (a, b, centre) = handles.plane(axis);
        let corner = |i: f32, j: f32| centre + (a * i + b * j) * half;
        let [p, q, r, s] = [
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ];
        for point in [p, q, r, p, r, s, p, r, q, p, s, r] {
            push(&mut vertices, point, axis.color());
        }
    }
    vertices
}

/// Rotate's white view ring, square to the camera. Painted before the axis
/// rings, which only ever stand inside it.
pub(in crate::renderer::gizmo) fn view_ring(handles: &Handles) -> Vec<Vertex> {
    let mut vertices = Vec::new();
    for step in 0..RING_SEGMENTS {
        tube_slice(
            &mut vertices,
            handles.origin(),
            handles.view_frame(),
            VIEW_RING_RADIUS * handles.arm(),
            RING_THICKNESS * handles.arm(),
            step,
            VIEW_COLOR,
        );
    }
    vertices
}

/// Repaints the classic axis colours every builder uses in `skin`'s.
pub(in crate::renderer::gizmo) fn recolor(vertices: &mut [Vertex], skin: Skin) {
    if skin == Skin::Classic {
        return;
    }
    for axis in Axis::ALL {
        let (from, to) = (axis.color(), skin.color(axis));
        for vertex in vertices.iter_mut().filter(|vertex| vertex.color == from) {
            vertex.color = to;
        }
    }
}
