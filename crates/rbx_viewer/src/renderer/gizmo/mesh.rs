//! The triangles one transform gizmo is made of.
//!
//! Where [`crate::gizmo`] says where the handles *are* — the half the editor's
//! UI thread hit-tests the cursor against — this is the half that turns those
//! same [`Handles`] into a mesh: Move's arrows, Scale's blocks, Rotate's rings.
//! Rebuilt every frame because the arms are scaled to keep a constant size on
//! screen and so change with every camera move.
//!
//! Everything here is drawn with the depth test off (a handle inside the part
//! it transforms still has to be grabbable), which leaves paint order as the
//! only thing deciding what covers what where two pieces overlap on screen.
//! Every function below therefore emits its pieces furthest from the eye
//! first, and winds every triangle counter-clockwise seen from outside so that
//! back-face culling removes the far side of each one.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::gizmo::{
    Axis, Handles, Kind, HANDLE_RADIUS, HEAD_RADIUS, HEAD_START, RING_RADIUS, RING_THICKNESS,
    SHAFT_RADIUS, SHAFT_START,
};

/// How many segments go round a shaft or an arrowhead. Eight already reads as
/// round at the size a dragger occupies on screen, and the whole gizmo is one
/// small vertex buffer rewritten every frame — there is nothing to gain from
/// more.
const SEGMENTS: usize = 8;
/// Three axes, each with an arm in both directions.
const ARMS: usize = 6;
/// Per arm: the shaft's sides and its open end's cap, then the arrowhead's
/// sides and base.
pub(super) const VERTICES_PER_ARM: usize = SEGMENTS * (6 + 3 + 3 + 3);
const ARROW_VERTICES: usize = ARMS * VERTICES_PER_ARM;

/// A cube: six faces of two triangles each.
const VERTICES_PER_BLOCK: usize = 36;
/// Per Scale arm: a shaft with its open end capped, then the block on its end.
const VERTICES_PER_HANDLE: usize = SEGMENTS * (6 + 3) + VERTICES_PER_BLOCK;
const HANDLE_VERTICES: usize = ARMS * VERTICES_PER_HANDLE;

/// How many slices go round one rotation ring. Far finer than a shaft needs:
/// a ring is a full circle the width of the whole gizmo, so its silhouette is
/// where a low segment count would show as an obvious polygon.
const RING_SEGMENTS: usize = 48;
/// How many segments go round a ring's tube.
const TUBE_SEGMENTS: usize = 6;
/// One slice of a ring: a band of quads all the way round the tube.
const VERTICES_PER_SLICE: usize = TUBE_SEGMENTS * 6;
const RING_VERTICES: usize = Axis::ALL.len() * RING_SEGMENTS * VERTICES_PER_SLICE;

const fn larger(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

/// The buffer has to hold whichever tool draws the most, since the tool
/// changes without the renderer being rebuilt.
pub(super) const CAPACITY: usize = larger(larger(ARROW_VERTICES, HANDLE_VERTICES), RING_VERTICES);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    pub(super) position: [f32; 3],
    pub(super) color: [f32; 3],
}

/// Named rather than written inline in [`Vertex::layout`]: a two-attribute
/// array is not const-promoted the way a one-attribute one is, so the
/// temporary would not outlive the layout that borrows it.
const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

impl Vertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// This tool's handles as one triangle list, painted back to front.
pub(super) fn mesh(kind: Kind, handles: &Handles, eye: Vec3) -> Vec<Vertex> {
    match kind {
        Kind::Move => arms(handles, eye, arrow),
        Kind::Scale => arms(handles, eye, handle),
        Kind::Rotate => rings(handles, eye),
    }
}

/// How one arm of an axis gizmo is built: into `vertices`, from `origin`,
/// pointing `direction`, `arm` studs long.
type Build = fn(&mut Vec<Vertex>, Vec3, Vec3, f32, [f32; 3]);

/// The six arms of one axis gizmo, furthest from `eye` first.
///
/// Sorting by arm is enough here: the arms never intersect each other, so a
/// painter's order over six convex pieces is exact rather than approximate.
fn arms(handles: &Handles, eye: Vec3, build: Build) -> Vec<Vertex> {
    let mut arms: Vec<(f32, Axis, f32)> = Axis::ALL
        .into_iter()
        .flat_map(|axis| [(axis, 1.0f32), (axis, -1.0f32)])
        .map(|(axis, sign)| {
            let tip = handles.origin() + handles.direction(axis) * handles.arm() * sign;
            ((tip - eye).length(), axis, sign)
        })
        .collect();
    arms.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(ARROW_VERTICES);
    for (_, axis, sign) in arms {
        build(
            &mut vertices,
            handles.origin(),
            handles.direction(axis) * sign,
            handles.arm(),
            axis.color(),
        );
    }
    vertices
}

/// A right-handed frame around an arm: `direction` is its own "up", and the
/// two returned vectors complete it (`across × round == direction`), which is
/// what makes a ring built from them run counter-clockwise seen from the tip.
fn frame(direction: Vec3) -> (Vec3, Vec3) {
    let across = direction.any_orthonormal_vector();
    (across, direction.cross(across))
}

/// One Move arrow: a thin shaft capped at the end nearest the gizmo's centre,
/// then a cone for the head.
fn arrow(vertices: &mut Vec<Vertex>, origin: Vec3, direction: Vec3, arm: f32, color: [f32; 3]) {
    shaft(vertices, origin, direction, arm, HEAD_START, color);

    let (across, round) = frame(direction);
    let neck = HEAD_START * arm;
    let ring = |segment: usize| {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        origin + direction * neck + (across * angle.cos() + round * angle.sin()) * HEAD_RADIUS * arm
    };
    let apex = origin + direction * arm;
    let base = origin + direction * neck;
    for segment in 0..SEGMENTS {
        let (a, b) = (ring(segment), ring(segment + 1));
        // The cone's side, then the disc it stands on — which faces back down
        // the shaft, hence the reversed winding.
        for point in [a, b, apex, base, b, a] {
            push(vertices, point, color);
        }
    }
}

/// One Scale handle: the same thin shaft with a block on its end instead of an
/// arrowhead.
///
/// `creator-docs` (`parts/index.md#transform-parts`) says only "click/drag a
/// handle" to "scale (resize) a selected part along the X, Y, or Z axis"; it
/// illustrates the blocks but never states their proportions, so these are
/// chosen to read at the same weight as the Move arrowhead rather than
/// measured from anything.
fn handle(vertices: &mut Vec<Vertex>, origin: Vec3, direction: Vec3, arm: f32, color: [f32; 3]) {
    // The block's outer face sits on the end of the arm, so a Scale gizmo
    // reaches exactly as far as a Move one does.
    let half = HANDLE_RADIUS * arm;
    let centre = origin + direction * (arm - half);
    shaft(vertices, origin, direction, arm, 1.0 - HANDLE_RADIUS, color);

    let (across, round) = frame(direction);
    let (a, b, c) = (across * half, round * half, direction * half);
    // `across × round == direction`, so (a, b, c) is right-handed and each
    // face below comes out wound away from the block's centre.
    for (offset, u, v) in [
        (c, a, b),
        (-c, b, a),
        (a, b, c),
        (-a, c, b),
        (b, c, a),
        (-b, a, c),
    ] {
        quad(vertices, centre + offset, u, v, color);
    }
}

/// The tube both axis tools hang their end piece on, capped at the end nearest
/// the gizmo's centre and open at the other, where the piece covers it.
/// `end` is in arm lengths, like every other proportion in [`crate::gizmo`];
/// the shaft always starts at `SHAFT_START`, which is the gap that leaves the
/// part itself clickable.
fn shaft(
    vertices: &mut Vec<Vertex>,
    origin: Vec3,
    direction: Vec3,
    arm: f32,
    end: f32,
    color: [f32; 3],
) {
    let (across, round) = frame(direction);
    let ring = |offset: f32, segment: usize| {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        origin
            + direction * offset * arm
            + (across * angle.cos() + round * angle.sin()) * SHAFT_RADIUS * arm
    };
    let base = origin + direction * SHAFT_START * arm;
    for segment in 0..SEGMENTS {
        let (a, b) = (ring(SHAFT_START, segment), ring(SHAFT_START, segment + 1));
        let (c, d) = (ring(end, segment), ring(end, segment + 1));
        // The side, as two triangles of one quad, then the open end facing
        // back towards the gizmo's centre — hence the reversed winding.
        for point in [a, b, d, a, d, c, base, b, a] {
            push(vertices, point, color);
        }
    }
}

/// The three rotation rings, as tori, painted furthest slice first.
///
/// Sorted per slice rather than per ring: three rings of the same radius about
/// one centre pass through each other's planes, so no order over whole rings
/// can be right. Each slice is a small convex piece, and the rings only ever
/// touch — never interpenetrate — so an order over slices is exact.
fn rings(handles: &Handles, eye: Vec3) -> Vec<Vertex> {
    let radius = RING_RADIUS * handles.arm();
    let mut slices: Vec<(f32, Axis, usize)> = Vec::with_capacity(Axis::ALL.len() * RING_SEGMENTS);
    for axis in Axis::ALL {
        let (_, zero, quarter) = handles.ring_frame(axis);
        for step in 0..RING_SEGMENTS {
            let angle = std::f32::consts::TAU * (step as f32 + 0.5) / RING_SEGMENTS as f32;
            let middle = handles.origin() + (zero * angle.cos() + quarter * angle.sin()) * radius;
            slices.push(((middle - eye).length(), axis, step));
        }
    }
    slices.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(RING_VERTICES);
    for (_, axis, step) in slices {
        slice(&mut vertices, handles, axis, step);
    }
    vertices
}

/// One slice of one ring: the band of the tube between two angles round the
/// ring.
fn slice(vertices: &mut Vec<Vertex>, handles: &Handles, axis: Axis, step: usize) {
    let (normal, zero, quarter) = handles.ring_frame(axis);
    let (radius, tube) = (RING_RADIUS * handles.arm(), RING_THICKNESS * handles.arm());
    let point = |step: usize, turn: usize| {
        let around = std::f32::consts::TAU * step as f32 / RING_SEGMENTS as f32;
        let through = std::f32::consts::TAU * turn as f32 / TUBE_SEGMENTS as f32;
        let out = zero * around.cos() + quarter * around.sin();
        handles.origin() + out * (radius + tube * through.cos()) + normal * (tube * through.sin())
    };

    let color = axis.color();
    for turn in 0..TUBE_SEGMENTS {
        // `(zero, quarter, normal)` is right-handed, so walking the ring and
        // then the tube in increasing order comes out wound outwards.
        let (a, b) = (point(step, turn), point(step + 1, turn));
        let (c, d) = (point(step + 1, turn + 1), point(step, turn + 1));
        for vertex in [a, b, c, a, c, d] {
            push(vertices, vertex, color);
        }
    }
}

/// A flat rectangle centred on `centre`, reaching `u` and `v` from it, wound
/// counter-clockwise seen from the `u × v` side.
fn quad(vertices: &mut Vec<Vertex>, centre: Vec3, u: Vec3, v: Vec3, color: [f32; 3]) {
    let corners = [
        centre - u - v,
        centre + u - v,
        centre + u + v,
        centre - u + v,
    ];
    for corner in [0, 1, 2, 0, 2, 3] {
        push(vertices, corners[corner], color);
    }
}

fn push(vertices: &mut Vec<Vertex>, position: Vec3, color: [f32; 3]) {
    vertices.push(Vertex {
        position: position.into(),
        color,
    });
}

#[cfg(test)]
#[path = "mesh/tests.rs"]
mod tests;
