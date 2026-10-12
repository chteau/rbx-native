//! The triangles one transform gizmo is made of.
//!
//! Where [`crate::gizmo`] says where the handles *are* — the half the editor's
//! UI thread hit-tests the cursor against — this is the half that turns that
//! same [`Shape`] into a mesh: Move's arrows, Scale's balls, Rotate's rings.
//! Rebuilt every frame because everything here is scaled to keep a constant
//! size on screen and so changes with every camera move.
//!
//! Everything here is drawn with the depth test off (a handle inside the part
//! it transforms still has to be grabbable), which leaves paint order as the
//! only thing deciding what covers what where two pieces overlap on screen.
//! Every function below therefore emits its pieces furthest from the eye
//! first, and winds every triangle counter-clockwise seen from outside so that
//! back-face culling removes the far side of each one.

mod balls;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::gizmo::{
    Axis, End, Faces, Handles, Hotspots, Shape, HEAD_RADIUS, HEAD_START, HOTSPOTS, HOTSPOT_PARTS,
    PART_HOTSPOTS, RING_RADIUS, RING_THICKNESS, SHAFT_RADIUS, SHAFT_START,
};
#[cfg(test)]
use balls::ORIGIN_COLOR;
use balls::{ball, balls, origin_ball};

/// How many segments go round a shaft or an arrowhead. Eight already reads as
/// round at the size a dragger occupies on screen, and the whole gizmo is one
/// small vertex buffer rewritten every frame — there is nothing to gain from
/// more.
const SEGMENTS: usize = 8;
/// Three axes, each with a dragger in both directions: an arm each way for
/// Move, a ball on each of the six faces for Scale.
const ARMS: usize = 6;
/// Per arm: the shaft's sides and its open end's cap, then the arrowhead's
/// sides and base.
pub(super) const VERTICES_PER_ARM: usize = SEGMENTS * (6 + 3 + 3 + 3);
const ARROW_VERTICES: usize = ARMS * VERTICES_PER_ARM;

/// How many segments go round a ball, and how many bands it is sliced into
/// from pole to pole. A Scale handle is a small blob on screen, but it is a
/// silhouette with no straight edge to hide behind, so it needs rather more
/// than a shaft does before it stops reading as a gem.
const BALL_SEGMENTS: usize = 12;
const BALL_BANDS: usize = 6;
/// Per ball: a triangle for each segment at each pole, where a whole ring of
/// the sphere collapses to a point, and a quad for every band between them.
pub(super) const VERTICES_PER_BALL: usize = BALL_SEGMENTS * (2 * 3 + (BALL_BANDS - 2) * 6);
const BALL_VERTICES: usize = ARMS * VERTICES_PER_BALL;

/// How many slices go round one rotation ring. Far finer than a shaft needs:
/// a ring is a full circle the width of the whole gizmo, so its silhouette is
/// where a low segment count would show as an obvious polygon.
const RING_SEGMENTS: usize = 48;
/// How many segments go round a ring's tube.
const TUBE_SEGMENTS: usize = 6;
/// One slice of a ring: a band of quads all the way round the tube.
const VERTICES_PER_SLICE: usize = TUBE_SEGMENTS * 6;
const RING_VERTICES: usize = Axis::ALL.len() * RING_SEGMENTS * VERTICES_PER_SLICE;

/// The buffer has to hold whichever tool draws the most, since the tool
/// changes without the renderer being rebuilt. Transform draws all three at
/// once, so it alone decides this — the other tools each need less.
/// The one extra ball is the free-drag handle at the Move gizmo's origin.
pub(super) const TRANSFORM_VERTICES: usize =
    ARROW_VERTICES + BALL_VERTICES + RING_VERTICES + VERTICES_PER_BALL;
/// Edit Pivot draws Transform's arms, rings and free-drag ball without its
/// Scale balls, and a dot per snap hotspot instead — the box's, and up to
/// `HOTSPOT_PARTS` parts' own.
pub(super) const PIVOT_VERTICES: usize = ARROW_VERTICES
    + RING_VERTICES
    + VERTICES_PER_BALL
    + (HOTSPOTS + HOTSPOT_PARTS * PART_HOTSPOTS) * VERTICES_PER_BALL;
pub(super) const CAPACITY: usize = if TRANSFORM_VERTICES > PIVOT_VERTICES {
    TRANSFORM_VERTICES
} else {
    PIVOT_VERTICES
};

/// Edit Pivot's snap hotspots: "small magenta points" (`creator-docs`,
/// `studio/pivot-tools.md`). Pure magenta is the same in sRGB and linear.
const HOTSPOT_COLOR: [f32; 3] = [1.0, 0.0, 1.0];
/// How much larger the hotspot the pivot has snapped onto is drawn.
const SNAPPED_SCALE: f32 = 2.2;

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

/// This tool's handles as one triangle list, painted back to front — only
/// the `held` one, while a drag holds a Move arrow or a Scale ball.
pub(super) fn mesh(shape: &Shape, held: Option<End>, eye: Vec3) -> Vec<Vertex> {
    let shown = |axis: Axis, sign: f32| held.is_none_or(|end| end.is(axis, sign));
    match shape {
        Shape::Move(handles) => arms(handles, eye, arrow, shown, held.is_none()),
        Shape::Scale(faces) => balls(faces, eye, shown),
        Shape::Rotate(handles) => rings(handles, eye),
        Shape::Transform(handles, faces) => transform(handles, faces, eye, shown, held.is_none()),
        Shape::Pivot(handles, hotspots) => {
            let mut vertices = rings(handles, eye);
            vertices.extend(arms(handles, eye, arrow, shown, held.is_none()));
            if let Some((hotspots, snapped)) = hotspots {
                dots(&mut vertices, hotspots, *snapped);
            }
            vertices
        }
    }
}

/// How far a hovered handle's colour moves towards white.
const LIT: f32 = 0.55;

/// Brightens every vertex painted in `axis`'s colour: the hovered arrow or
/// ring (and, under Transform, the rest of that axis's handles, which share
/// its colour). Handles are flat-coloured, so the colour alone names them.
pub(super) fn light(vertices: &mut [Vertex], axis: Axis) {
    let color = axis.color();
    let lit = color.map(|channel| channel + (1.0 - channel) * LIT);
    for vertex in vertices.iter_mut().filter(|vertex| vertex.color == color) {
        vertex.color = lit;
    }
}

/// Edit Pivot's hotspots, painted last: dots far smaller than any handle,
/// which only ever sit on the selection's boxes, so being drawn over the
/// arms and rings costs nothing and keeps every one of them visible.
fn dots(vertices: &mut Vec<Vertex>, hotspots: &Hotspots, snapped: Option<Vec3>) {
    for point in hotspots.points() {
        let on = snapped.is_some_and(|snapped| (snapped - point).length() < 1e-4);
        let scale = if on { SNAPPED_SCALE } else { 1.0 };
        ball(
            vertices,
            point,
            hotspots.radius(point) * scale,
            HOTSPOT_COLOR,
        );
    }
}

/// One piece of Transform's combined gizmo, carrying enough to build it once
/// its turn in the painter's order comes up.
enum Piece {
    Arm(Axis, f32),
    Ball(Axis, f32),
    Ring(Axis, usize),
    Origin,
}

/// Transform's arms, balls and rings as one triangle list, sorted back to
/// front across all three kinds together.
///
/// [`arms`], [`balls`] and [`rings`] each already paint their own six (or,
/// for a ring, `RING_SEGMENTS`) pieces furthest-from-`eye` first, but that
/// sort never looked outside its own kind — which was fine while only one
/// kind was ever on screen at once. Transform is the first shape where that
/// stops being true: a ball nearer the eye than some arm has to be painted
/// after it regardless of which kind either belongs to, or the depth-test-off
/// painter's algorithm every handle relies on (see this module's own doc
/// comment) paints the far arm over the near ball.
fn transform(
    handles: &Handles,
    faces: &Faces,
    eye: Vec3,
    shown: impl Fn(Axis, f32) -> bool,
    origin: bool,
) -> Vec<Vertex> {
    let mut pieces: Vec<(f32, Piece)> =
        Vec::with_capacity(ARMS + ARMS + Axis::ALL.len() * RING_SEGMENTS + 1);
    if origin {
        pieces.push(((handles.origin() - eye).length(), Piece::Origin));
    }
    for axis in Axis::ALL {
        for sign in [1.0f32, -1.0] {
            if shown(axis, sign) {
                let tip = handles.origin() + handles.direction(axis) * handles.arm() * sign;
                pieces.push(((tip - eye).length(), Piece::Arm(axis, sign)));
                pieces.push((
                    (faces.handle(axis, sign) - eye).length(),
                    Piece::Ball(axis, sign),
                ));
            }
        }
        let (_, zero, quarter) = handles.ring_frame(axis);
        let radius = RING_RADIUS * handles.arm();
        for step in 0..RING_SEGMENTS {
            let angle = std::f32::consts::TAU * (step as f32 + 0.5) / RING_SEGMENTS as f32;
            let middle = handles.origin() + (zero * angle.cos() + quarter * angle.sin()) * radius;
            pieces.push(((middle - eye).length(), Piece::Ring(axis, step)));
        }
    }
    pieces.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(CAPACITY);
    for (_, piece) in pieces {
        match piece {
            Piece::Arm(axis, sign) => arrow(
                &mut vertices,
                handles.origin(),
                handles.direction(axis) * sign,
                handles.arm(),
                axis.color(),
            ),
            Piece::Ball(axis, sign) => ball(
                &mut vertices,
                faces.handle(axis, sign),
                faces.radius(axis, sign),
                axis.color(),
            ),
            Piece::Ring(axis, step) => slice(&mut vertices, handles, axis, step),
            Piece::Origin => origin_ball(&mut vertices, handles),
        }
    }
    vertices
}

/// How one arm of an axis gizmo is built: into `vertices`, from `origin`,
/// pointing `direction`, `arm` studs long.
type Build = fn(&mut Vec<Vertex>, Vec3, Vec3, f32, [f32; 3]);

/// The six arms of one axis gizmo, furthest from `eye` first, with the
/// free-drag ball at their origin among them when `origin` is set.
///
/// Sorting by arm is enough here: the arms never intersect each other or the
/// ball, so a painter's order over these convex pieces is exact rather than
/// approximate.
fn arms(
    handles: &Handles,
    eye: Vec3,
    build: Build,
    shown: impl Fn(Axis, f32) -> bool,
    origin: bool,
) -> Vec<Vertex> {
    let mut arms: Vec<(f32, Option<(Axis, f32)>)> = Axis::ALL
        .into_iter()
        .flat_map(|axis| [(axis, 1.0f32), (axis, -1.0f32)])
        .filter(|&(axis, sign)| shown(axis, sign))
        .map(|(axis, sign)| {
            let tip = handles.origin() + handles.direction(axis) * handles.arm() * sign;
            ((tip - eye).length(), Some((axis, sign)))
        })
        .collect();
    if origin {
        arms.push(((handles.origin() - eye).length(), None));
    }
    arms.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(ARROW_VERTICES + VERTICES_PER_BALL);
    for (_, arm) in arms {
        let Some((axis, sign)) = arm else {
            origin_ball(&mut vertices, handles);
            continue;
        };
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
    shaft(vertices, origin, direction, arm, color);

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

/// The tube a Move arrow hangs its head on, capped at the end nearest the
/// gizmo's centre and open at the other, where the head covers it. The shaft
/// starts at `SHAFT_START`, which is the gap that leaves the part itself
/// clickable, and ends where the head begins.
fn shaft(vertices: &mut Vec<Vertex>, origin: Vec3, direction: Vec3, arm: f32, color: [f32; 3]) {
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
        let (c, d) = (ring(HEAD_START, segment), ring(HEAD_START, segment + 1));
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

fn push(vertices: &mut Vec<Vertex>, position: Vec3, color: [f32; 3]) {
    vertices.push(Vertex {
        position: position.into(),
        color,
    });
}

#[cfg(test)]
#[path = "mesh/tests.rs"]
mod tests;
