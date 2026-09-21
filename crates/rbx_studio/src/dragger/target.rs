//! The target frame for every kind of part (`DragHelper.getSurfaceMatrix`):
//! a box or wedge face measured from its corner nearest the cursor, a ball's
//! latitude and longitude, a cylinder's cap or side, and a mesh's face
//! measured from the edge Studio probes for.
//!
//! The ball, the cylinder's side and the fallback for a mesh with no edge
//! snap *inside* the frame — the grid is already in where the frame stands —
//! so those need the grid in force; a face's frame does not.

mod mesh;

use std::f32::consts::FRAC_PI_2;

use glam::{Mat3, Mat4, Vec2, Vec3};
use rbx_viewer::pick::{PartSurface, Ray, Solid};

use super::surface::{surface_frame, SurfaceFrame, TargetKind};
use super::{sign, snap_to};

/// The frame of `part`'s surface at `hit`, where the raycast that found it
/// reported `normal`, with `grid` in force (`0.0` for none: off, or `Shift`).
pub(crate) fn target_frame(
    part: &PartSurface,
    hit: Vec3,
    normal: Vec3,
    grid: f32,
) -> Option<SurfaceFrame> {
    let frame = match part.solid {
        Solid::Box => surface_frame(part.model, hit)?,
        Solid::Wedge => polygon(part.model, hit, normal, &WEDGE, &WEDGE_EDGES)?,
        Solid::CornerWedge => polygon(part.model, hit, normal, &CORNER_WEDGE, &CORNER_WEDGE_EDGES)?,
        Solid::Ball => ball(part.model, hit, grid)?,
        Solid::Cylinder => cylinder(part.model, hit, normal, grid)?,
        Solid::Mesh => match mesh::closest_edge(part, hit, normal) {
            Some((a, b)) => mesh_edge(part.model, hit, normal, a, b)?,
            None => round(part.model, hit, normal, grid)?,
        },
    };
    let round_part =
        matches!(part.solid, Solid::Ball | Solid::Cylinder).then_some((part.solid, part.model));
    Some(SurfaceFrame {
        part: round_part,
        ..frame
    })
}

/// The frame under `ray` on `part`, and the point on it a ruler measures
/// to: where the ray meets the part, or on a surface with no edge — whose
/// frame already stands on the snapped point — that point.
pub(crate) fn under(part: &PartSurface, ray: Ray, grid: f32) -> Option<(SurfaceFrame, Vec3)> {
    let (distance, normal) = part.raycast(ray)?;
    let hit = ray.at(distance);
    let frame = target_frame(part, hit, normal, grid)?;
    let point = if frame.kind == TargetKind::Round {
        frame.corner
    } else {
        hit
    };
    Some((frame, point))
}

/// A part's rigid placement and its size, out of the box it is drawn in.
pub(super) fn placement(model: Mat4) -> Option<(Mat3, Vec3, Vec3)> {
    let columns = [model.x_axis, model.y_axis, model.z_axis].map(|column| column.truncate());
    let size = Vec3::from(columns.map(|column| column.length()));
    let rotation = Mat3::from_cols(
        columns[0].try_normalize()?,
        columns[1].try_normalize()?,
        columns[2].try_normalize()?,
    );
    Some((rotation, model.w_axis.truncate(), size))
}

/// Studio's `fromMatrix(origin, -(edge × n), n)`: y the normal, z along the
/// edge, x across it.
fn frame(
    origin: Vec3,
    normal: Vec3,
    edge: Vec3,
    size: Vec2,
    kind: TargetKind,
) -> Option<SurfaceFrame> {
    let x = normal.cross(edge).try_normalize()?;
    Some(SurfaceFrame {
        corner: origin,
        x,
        y: normal,
        // Square to the normal even where the edge is not (a corner wedge's
        // diagonal seen from its floor), which is what `fromMatrix` leaves.
        z: x.cross(normal),
        size,
        kind,
        part: None,
    })
}

/// `getSizeInSurface`: the whole box of `model` measured along `x` and `z`.
fn size_in(model: Mat4, x: Vec3, z: Vec3) -> Vec2 {
    let reach = |axis: Vec3| {
        (0..3)
            .map(|column| model.col(column).truncate().dot(axis).abs())
            .sum::<f32>()
    };
    Vec2::new(reach(x), reach(z))
}

// `getGeometry`'s vertex and edge tables, in the part's unit box.
const WEDGE: [Vec3; 6] = [
    Vec3::new(0.5, 0.5, 0.5),
    Vec3::new(-0.5, 0.5, 0.5),
    Vec3::new(0.5, -0.5, 0.5),
    Vec3::new(0.5, -0.5, -0.5),
    Vec3::new(-0.5, -0.5, 0.5),
    Vec3::new(-0.5, -0.5, -0.5),
];
const WEDGE_EDGES: [(usize, usize); 9] = [
    (0, 1),
    (0, 3),
    (1, 5),
    (2, 3),
    (4, 5),
    (2, 4),
    (3, 5),
    (0, 2),
    (1, 4),
];
const CORNER_WEDGE: [Vec3; 5] = [
    Vec3::new(0.5, 0.5, -0.5),
    Vec3::new(0.5, -0.5, 0.5),
    Vec3::new(0.5, -0.5, -0.5),
    Vec3::new(-0.5, -0.5, 0.5),
    Vec3::new(-0.5, -0.5, -0.5),
];
const CORNER_WEDGE_EDGES: [(usize, usize); 8] = [
    (1, 2),
    (2, 4),
    (4, 3),
    (3, 1),
    (0, 2),
    (0, 1),
    (0, 4),
    (0, 3),
];

/// A wedge's face (DragHelper PROTO_18): of every edge of the part — Studio's
/// edge test is signed, so on a convex part none is ever filtered out — the
/// one whose line runs nearest `hit`, cornered on its end nearest `hit`.
fn polygon(
    model: Mat4,
    hit: Vec3,
    normal: Vec3,
    vertices: &[Vec3],
    edges: &[(usize, usize)],
) -> Option<SurfaceFrame> {
    let world: Vec<Vec3> = vertices
        .iter()
        .map(|&v| model.transform_point3(v))
        .collect();
    let line_distance = |(a, b): (usize, usize)| {
        let (a, b) = (world[a], world[b]);
        let along = (b - a).normalize_or_zero();
        let offset = hit - a;
        (offset - along * offset.dot(along)).length()
    };
    let &(a, b) = edges
        .iter()
        .filter(|&&(a, b)| normal.cross(world[b] - world[a]).length_squared() > 1e-12)
        .min_by(|&&x, &&y| line_distance(x).total_cmp(&line_distance(y)))?;
    let (a, b) = (world[a], world[b]);
    let origin = if (a - hit).length() <= (b - hit).length() {
        a
    } else {
        b
    };
    let edge = (b - a).normalize();
    let bare = frame(origin, normal, edge, Vec2::ZERO, TargetKind::Polygon)?;
    Some(SurfaceFrame {
        size: size_in(model, bare.x, bare.z),
        ..bare
    })
}

/// A mesh face (DragHelper PROTO_15): cornered on the nearer end of the
/// probed edge, its size the part's box in that frame, capped at the edge's
/// length.
fn mesh_edge(model: Mat4, hit: Vec3, normal: Vec3, a: Vec3, b: Vec3) -> Option<SurfaceFrame> {
    let origin = if (a - hit).length() < (b - hit).length() {
        a
    } else {
        b
    };
    let length = (b - a).length();
    let bare = frame(
        origin,
        normal,
        (b - a) / length,
        Vec2::ZERO,
        TargetKind::Polygon,
    )?;
    Some(SurfaceFrame {
        size: size_in(model, bare.x, bare.z).min(Vec2::splat(length)),
        ..bare
    })
}

/// A ball (DragHelper PROTO_11): latitude snapped as a height up the ball,
/// longitude by arc length along the latitude ring from the nearest meridian
/// on an axis; the frame stands on the ball at that point, facing out, with
/// z along the ring. Near either pole it becomes a flat point at the pole.
fn ball(model: Mat4, hit: Vec3, grid: f32) -> Option<SurfaceFrame> {
    let (rotation, centre, size) = placement(model)?;
    let local = rotation.transpose() * (hit - centre);
    let radius = 0.5 * size.min_element();
    let mut height = snap_to(local.y, grid);
    let mut ring = if radius < height.abs() {
        0.0
    } else {
        (radius * radius - height * height).sqrt()
    };
    let pole = ring < 0.25 * radius || Vec2::new(local.x, local.z).length() < 0.25 * radius;
    if pole {
        ring = 0.0;
        height = sign(local.y) * radius;
    }
    let (sx, sz) = (sign(local.x), sign(local.z));
    let ((a, b), direction) = (
        if local.z.abs() >= local.x.abs() {
            (Vec2::new(0.0, sz * ring), Vec2::new(sx * ring, 0.0))
        } else {
            (Vec2::new(sx * ring, 0.0), Vec2::new(0.0, sz * ring))
        },
        Vec2::new(local.x, local.z).normalize_or_zero(),
    );
    let around = arc(a, b, direction, ring, grid);
    let on_ball = Vec3::new(around.x, height, around.y);
    let normal = (rotation * on_ball).try_normalize()?;
    let (right, up, back) = (rotation.x_axis, rotation.y_axis, rotation.z_axis);
    let along = if normal.dot(up).abs() > 0.99 {
        normal
            .cross(right)
            .try_normalize()
            .or_else(|| normal.cross(back).try_normalize())
    } else {
        normal.cross(up).try_normalize()
    }?;
    let kind = if pole {
        TargetKind::Polygon
    } else {
        TargetKind::Sphere
    };
    frame(centre + rotation * on_ball, normal, along, Vec2::ZERO, kind)
}

/// The point `ring` round a circle from `a` towards `b` (two points a
/// quarter-turn apart on it) by the arc length the cursor's `direction` is
/// round, snapped to `grid`: `a·sin((1-t)π/2) + b·sin(tπ/2)`.
fn arc(a: Vec2, b: Vec2, direction: Vec2, ring: f32, grid: f32) -> Vec2 {
    if ring <= 0.0 {
        return Vec2::ZERO;
    }
    let angle = (a.dot(direction) / ring).clamp(-1.0, 1.0).acos();
    let angle = if angle.is_nan() { 0.0 } else { angle };
    let t = snap_to(angle * ring, grid) / (FRAC_PI_2 * ring);
    a * ((1.0 - t) * FRAC_PI_2).sin() + b * (t * FRAC_PI_2).sin()
}

/// A cylinder (DragHelper PROTO_12), lying along its own X. On a cap: a face
/// `r` square cornered on the centre, a rim point on an axis, or a corner of
/// the square round the cap, whichever the cursor is nearest. On the side:
/// the rim point at the end nearest the cursor, round by the snapped arc
/// length, facing out, with z along the cylinder.
fn cylinder(model: Mat4, hit: Vec3, normal: Vec3, grid: f32) -> Option<SurfaceFrame> {
    let (rotation, centre, size) = placement(model)?;
    let local = rotation.transpose() * (hit - centre);
    let radius = 0.5 * size.y.min(size.z);
    let half = 0.5 * size.x;
    let (right, up) = (rotation.x_axis, rotation.y_axis);
    if (local.x.abs() - half).abs() < 0.001 {
        let offset = |v: f32| {
            if v.abs() > 0.5 * radius {
                sign(v) * radius
            } else {
                0.0
            }
        };
        let anchor = Vec3::new(sign(local.x) * half, offset(local.y), offset(local.z));
        return frame(
            centre + rotation * anchor,
            normal,
            up * sign(local.x),
            Vec2::splat(radius),
            TargetKind::Polygon,
        );
    }
    let (sy, sz) = (sign(local.y), sign(local.z));
    let (a, b) = if local.z.abs() >= local.y.abs() {
        (Vec2::new(0.0, sz * radius), Vec2::new(sy * radius, 0.0))
    } else {
        (Vec2::new(sy * radius, 0.0), Vec2::new(0.0, sz * radius))
    };
    let direction = Vec2::new(local.y, local.z) / radius;
    let around = arc(a, b, direction, radius, grid);
    let out = (rotation * Vec3::new(0.0, around.x, around.y)).try_normalize()?;
    let rim = Vec3::new(half * sign(local.x), around.x, around.y);
    frame(
        centre + rotation * rim,
        out,
        right,
        Vec2::new(0.0, size.x),
        TargetKind::Cylinder,
    )
}

/// What a surface with no edge to measure from gets (DragHelper PROTO_14):
/// the hit, snapped to the part's own lattice and put back on the tangent
/// plane, facing out.
fn round(model: Mat4, hit: Vec3, normal: Vec3, grid: f32) -> Option<SurfaceFrame> {
    let (rotation, centre, _) = placement(model)?;
    let point = if grid > 0.0 {
        on_lattice(hit, normal, grid, rotation, centre)
    } else {
        hit
    };
    let mut across = rotation.x_axis;
    if across.dot(normal).abs() > 0.9 {
        across = rotation.z_axis;
    }
    let mut w = across.cross(normal);
    for fallback in [Vec3::X, Vec3::Y, Vec3::Z] {
        if w.length() >= 1e-5 {
            break;
        }
        w = fallback.cross(normal);
    }
    let x = w.normalize().cross(normal);
    Some(SurfaceFrame {
        corner: point,
        x,
        y: normal,
        z: x.cross(normal),
        size: Vec2::ZERO,
        kind: TargetKind::Round,
        part: None,
    })
}

/// `snapToLattice` (DragHelper PROTO_13): the lattice point nearest `hit` in
/// the frame (`rotation`, `origin`), pushed back onto the plane through `hit`
/// along whichever lattice axis needs the least travel.
fn on_lattice(hit: Vec3, normal: Vec3, grid: f32, rotation: Mat3, origin: Vec3) -> Vec3 {
    let inverse = rotation.transpose();
    let local = inverse * (hit - origin);
    let snapped = local.map(|v| snap_to(v, grid));
    let plane = inverse * normal;
    let travel = |axis: Vec3| {
        let t = (local - snapped).dot(plane) / axis.dot(plane);
        if t.is_finite() {
            t
        } else {
            f32::INFINITY
        }
    };
    let axis = [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .min_by(|&a, &b| travel(a).abs().total_cmp(&travel(b).abs()))
        .unwrap_or(Vec3::Y);
    let t = travel(axis);
    let local = if t.is_finite() {
        snapped + axis * t
    } else {
        snapped
    };
    origin + rotation * local
}

#[cfg(test)]
#[path = "target/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "target/cost.rs"]
mod cost;
