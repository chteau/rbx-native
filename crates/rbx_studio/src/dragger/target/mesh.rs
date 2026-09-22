//! `blackboxFindClosestMeshEdge`: the edge of a mesh face nearest a hit,
//! found the way Studio finds it — not from the mesh's triangles, which its
//! Lua cannot read, but by probing the part with raycasts. Walking out across
//! the face in four directions until the surface falls away or a wall rises
//! finds the neighbouring face; the two faces' planes meet in the edge's
//! line; walking along that line the same way finds where the edge ends.
//! Every raycast is against the one part, as Studio's whitelist has it.
//!
//! ponytail: each probe walks every triangle of the mesh, and a target frame
//! takes 25-100 of them — at most 0.95 ms per hover on the largest mesh of the
//! fixture places (release, `target::cost`). A per-mesh BVH is the upgrade if
//! a denser mesh pushes that past a frame's budget.

use glam::Vec3;
use rbx_viewer::pick::{PartSurface, Ray};

/// How far above the face the probes start, and how far down a probe looks
/// for the face still being there.
const LIFT: f32 = 0.01;
const DROP: f32 = 0.02;
/// The march's first step, how much each step grows, and how many it takes.
const FIRST_STEP: f32 = 0.01;
const GROWTH: f32 = 2.28;
const STEPS: usize = 14;
/// A neighbouring face counts when it turns at least this far from the one
/// hit (`Normal·n < 0.5`); an edge's end when the face met turns this far from
/// the edge's bisector.
const TURN: f32 = 0.5;
const END_TURN: f32 = 0.8;

/// One raycast against `part`: the hit point and its normal, within `reach`.
fn cast(
    part: &PartSurface,
    origin: Vec3,
    direction: Vec3,
    reach: f32,
) -> Option<(Vec3, Vec3, f32)> {
    let unit = direction.try_normalize()?;
    let (distance, normal) = part.raycast(Ray::new(origin, unit))?;
    (distance <= reach).then(|| (origin + unit * distance, normal, distance))
}

/// The ends of the edge of `part`'s face through `hit` nearest it, or `None`
/// where no neighbouring face turns sharply enough (a smooth, curved mesh) or
/// the edge runs on without end.
pub(super) fn closest_edge(part: &PartSurface, hit: Vec3, normal: Vec3) -> Option<(Vec3, Vec3)> {
    let size = Vec3::new(
        part.model.x_axis.truncate().length(),
        part.model.y_axis.truncate().length(),
        part.model.z_axis.truncate().length(),
    );
    let reach = size.length() + 0.01;
    let neighbour = neighbour(part, hit, normal, reach)?;
    let (point, direction, towards_hit, towards_neighbour) = crease(hit, normal, neighbour)?;
    let bisector = (normal + neighbour.1).try_normalize()?;
    let start = point + bisector * LIFT;
    let along = |sign: f32| {
        end_along(
            part,
            start,
            point,
            direction * sign,
            [towards_hit, towards_neighbour],
            bisector,
            reach,
        )
    };
    let (far, near) = (along(1.0)?, along(-1.0)?);
    let (a, b) = (point + direction * far, point - direction * near);
    ((a - b).length() > 1e-4).then_some((a, b))
}

/// The nearest face turning away from the one hit, walking out across it in
/// four directions (PROTO_4): a wall met head-on, or the face met from below
/// once the walk has stepped off an edge.
fn neighbour(part: &PartSurface, hit: Vec3, normal: Vec3, reach: f32) -> Option<(Vec3, Vec3)> {
    let (rotation_x, rotation_y) = (
        part.model.x_axis.truncate().normalize_or_zero(),
        part.model.y_axis.truncate().normalize_or_zero(),
    );
    let axis = if rotation_x.cross(normal).length() < 0.01 {
        rotation_y
    } else {
        rotation_x
    };
    let v = axis.cross(normal).try_normalize()?;
    let u = normal.cross(v);
    let above = hit + normal * LIFT;
    let mut best: Option<((Vec3, Vec3), f32)> = None;
    let mut keep = |candidate: (Vec3, Vec3), distance: f32| {
        if best.is_none_or(|(_, held)| distance < held) {
            best = Some((candidate, distance));
        }
    };
    for direction in [v, u, -v, -u] {
        // A wall rising ahead (a concave edge).
        let wall = cast(part, above, direction, reach);
        let wall_distance = wall.map_or(f32::INFINITY, |(.., distance)| distance);
        if let Some((point, facing, distance)) = wall {
            if facing.dot(normal) < TURN {
                keep((point, facing), distance);
            }
        }
        // The face falling away (a convex edge).
        let mut step = FIRST_STEP;
        for _ in 0..STEPS {
            if step > wall_distance {
                break;
            }
            let probe = above + direction * step;
            if cast(part, probe, -normal, DROP).is_some() {
                step *= GROWTH;
                continue;
            }
            let below = probe - normal * DROP;
            if let Some((point, facing, _)) = cast(part, below, hit - below, reach) {
                if facing.dot(normal) < TURN {
                    keep((point, facing), (point - above).length());
                }
            }
            break;
        }
    }
    best.map(|(candidate, _)| candidate)
}

/// Where the plane through `hit` facing `normal` meets the neighbour's
/// (PROTO_0): a point on both, the line's direction, and the unit directions
/// in each plane from the line towards `hit` and towards the neighbour.
fn crease(
    hit: Vec3,
    normal: Vec3,
    (other, facing): (Vec3, Vec3),
) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let line = normal.cross(facing);
    let length = line.length_squared();
    if length < 1e-12 {
        return None;
    }
    let (d1, d2) = (normal.dot(hit), facing.dot(other));
    let point = (facing.cross(line) * d1 + line.cross(normal) * d2) / length;
    let direction = line / length.sqrt();
    let towards = |plane_normal: Vec3, target: Vec3| {
        let inward = direction.cross(plane_normal).normalize_or_zero();
        if inward.dot(target - point) < 0.0 {
            -inward
        } else {
            inward
        }
    };
    Some((
        point,
        direction,
        towards(normal, hit),
        towards(facing, other),
    ))
}

/// How far along `direction` from `point` the edge runs before it ends
/// (PROTO_2): walking along it just outside, until the surface under the walk
/// falls away onto a face turning from the bisector, or a face ahead cuts the
/// line off first.
fn end_along(
    part: &PartSurface,
    start: Vec3,
    point: Vec3,
    direction: Vec3,
    inward: [Vec3; 2],
    bisector: Vec3,
    reach: f32,
) -> Option<f32> {
    // Where a face's plane crosses the edge's line, as a distance along it.
    let crossing = |(at, facing): (Vec3, Vec3)| {
        let rate = direction.dot(facing);
        (rate.abs() > 1e-6).then(|| (at - point).dot(facing) / rate)
    };
    let mut limit = inward
        .iter()
        .filter_map(|&side| cast(part, start + side * LIFT, direction, reach))
        .filter_map(|(at, facing, _)| crossing((at, facing)))
        .fold(f32::INFINITY, f32::min);
    let mut step = FIRST_STEP;
    for _ in 0..STEPS {
        if step > limit {
            break;
        }
        let probe = start + direction * step;
        if cast(part, probe, -bisector, DROP).is_some() {
            step *= GROWTH;
            continue;
        }
        let below = probe - bisector * DROP;
        if let Some((at, facing, _)) = cast(part, below, point - below, reach) {
            if facing.dot(bisector) < END_TURN {
                if let Some(t) = crossing((at, facing)) {
                    limit = limit.min(t);
                }
            }
        }
        break;
    }
    limit.is_finite().then_some(limit)
}
