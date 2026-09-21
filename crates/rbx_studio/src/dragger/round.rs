//! What Studio draws on a ball or a cylinder, which have no face corner for
//! a ruler to start from: the part's major lines (`SphereMajorLines`,
//! `CylinderMajorLines`), and for a drag landing on one, the latitude and
//! longitude through the snapped point (`LatLonGuide`), the lattice round a
//! pole or along a cylinder's side (`Grid3D`) and the ring round a cylinder
//! where the drag lands (`TargetGridView`).
//!
//! All of it is drawn in the part's own frame, at its true radius — half its
//! smallest size for a ball, half the smaller of Y and Z for a cylinder.

use glam::{Mat3, Vec2, Vec3};
use rbx_viewer::pick::Solid;

use super::surface::{SurfaceFrame, TargetKind};
use super::target::placement;
use super::{snap_to, Dot, Guides, Line, ACTIVE, PASSIVE};

mod lattice;

use lattice::{thinned, Lattice};

/// The centre point's radius, in handle scales.
const CENTRE_RADIUS: f32 = 0.15;

/// A part's rigid frame and size.
struct Part {
    rotation: Mat3,
    centre: Vec3,
    size: Vec3,
}

impl Part {
    fn world(&self, local: Vec3) -> Vec3 {
        self.centre + self.rotation * local
    }

    fn local(&self, world: Vec3) -> Vec3 {
        self.rotation.transpose() * (world - self.centre)
    }
}

/// Studio's circle: the 28 points round the square `[-3, 3]²`, seven a
/// side, pushed out onto the unit circle — 24 distinct, spaced 18.4° at the
/// axes and 11.3° at the diagonals.
fn circle() -> Vec<Vec2> {
    let side = || -3..=3;
    side()
        .map(|i| Vec2::new(-3.0, i as f32))
        .chain(side().map(|i| Vec2::new(i as f32, 3.0)))
        .chain(side().rev().map(|i| Vec2::new(3.0, i as f32)))
        .chain(side().rev().map(|i| Vec2::new(i as f32, -3.0)))
        .map(Vec2::normalize)
        .collect()
}

/// The segments between consecutive points, and back to the first when
/// `closed`; the repeated corners add none.
fn path(points: &[Vec3], closed: bool) -> Vec<[Vec3; 2]> {
    let mut pairs: Vec<[Vec3; 2]> = points.windows(2).map(|pair| [pair[0], pair[1]]).collect();
    if let (true, Some(&first), Some(&last)) = (closed, points.first(), points.last()) {
        pairs.push([last, first]);
    }
    pairs.retain(|[a, b]| a.distance_squared(*b) > 1e-12);
    pairs
}

/// A circle of `part`'s, radius `radius`, each point placed by `at`.
fn ring(part: &Part, radius: f32, at: impl Fn(Vec2) -> Vec3) -> Vec<[Vec3; 2]> {
    let points: Vec<Vec3> = circle()
        .into_iter()
        .map(|v| part.world(at(v * radius)))
        .collect();
    path(&points, true)
}

/// Lines in one style. One with no length draws nothing in Studio, where
/// the line layer would draw it as a dot, so it is left out.
fn styled(pairs: Vec<[Vec3; 2]>, color: [f32; 3], under: f32, over: f32) -> Vec<Line> {
    pairs
        .into_iter()
        .filter(|[from, to]| from.distance_squared(*to) > 1e-12)
        .map(|[from, to]| Line::hairline(from, to, color, under, over))
        .collect()
}

/// A major line: the grid colour, depth-tested, opaque.
fn major(pairs: Vec<[Vec3; 2]>) -> Vec<Line> {
    styled(pairs, PASSIVE, 1.0, 0.0)
}

/// A guide in the chosen colour over everything.
fn chosen(pairs: Vec<[Vec3; 2]>) -> Vec<Line> {
    styled(pairs, ACTIVE, 0.0, 1.0)
}

/// A ball's radius, half its smallest size.
fn ball_radius(part: &Part) -> f32 {
    0.5 * part.size.min_element()
}

/// A cylinder's radius and half its length along its own X.
fn cylinder_extent(part: &Part) -> (f32, f32) {
    (0.5 * part.size.y.min(part.size.z), 0.5 * part.size.x)
}

/// `SphereMajorLines` or `CylinderMajorLines` for the ball or cylinder
/// `frame` stands on; nothing for any other part. A ball's are its three
/// great circles; a cylinder's, the four lines along its side at ±Y and ±Z,
/// one ring at mid-length and a cross of diameters on each end.
pub(crate) fn major_lines(frame: &SurfaceFrame) -> Vec<Line> {
    let Some((solid, model)) = frame.part else {
        return Vec::new();
    };
    let Some((rotation, centre, size)) = placement(model) else {
        return Vec::new();
    };
    let part = Part {
        rotation,
        centre,
        size,
    };
    let mut pairs = Vec::new();
    match solid {
        Solid::Ball => {
            let r = ball_radius(&part);
            pairs.extend(ring(&part, r, |v| Vec3::new(0.0, v.x, v.y)));
            pairs.extend(ring(&part, r, |v| Vec3::new(v.x, v.y, 0.0)));
            pairs.extend(ring(&part, r, |v| Vec3::new(v.x, 0.0, v.y)));
        }
        Solid::Cylinder => {
            let (r, h) = cylinder_extent(&part);
            let offsets = [
                Vec3::new(0.0, r, 0.0),
                Vec3::new(0.0, -r, 0.0),
                Vec3::new(0.0, 0.0, r),
                Vec3::new(0.0, 0.0, -r),
            ];
            let end = Vec3::new(h, 0.0, 0.0);
            for offset in offsets {
                pairs.push([part.world(end + offset), part.world(offset - end)]);
            }
            pairs.extend(ring(&part, r, |v| Vec3::new(0.0, v.x, v.y)));
            for end in [end, -end] {
                pairs.push([part.world(end + offsets[0]), part.world(end + offsets[1])]);
                pairs.push([part.world(end + offsets[2]), part.world(end + offsets[3])]);
            }
        }
        _ => {}
    }
    major(pairs)
}

/// `cornerVectors`: `frame`'s in-plane axes each flipped towards `hit`, and
/// how far along each `hit` is from the frame's origin (never negative).
fn corner_vectors(frame: &SurfaceFrame, hit: Vec3) -> (Vec3, Vec3, f32, f32) {
    let relative = hit - frame.corner;
    let toward = |axis: Vec3| {
        if relative.dot(axis) < 0.0 {
            -axis
        } else {
            axis
        }
    };
    let (x, z) = (toward(frame.x), toward(frame.z));
    (x, z, relative.dot(x).max(0.0), relative.dot(z).max(0.0))
}

/// What a free drag landing on a ball or a cylinder draws in place of the
/// ruler (`TargetGridView`), with `hit` where the cursor meets the target
/// and `grid` the grid in force; `None` for any other part. `scale` is the
/// handle scale at a point. With a soft snap taken only the major lines
/// show, beside the alignment lines.
pub(crate) fn landed(
    frame: &SurfaceFrame,
    hit: Vec3,
    grid: f32,
    soft: bool,
    scale: impl Fn(Vec3) -> f32,
) -> Option<Guides> {
    let (solid, model) = frame.part?;
    let (rotation, centre, size) = placement(model)?;
    let part = Part {
        rotation,
        centre,
        size,
    };
    let mut guides = Guides {
        lines: major_lines(frame),
        dots: Vec::new(),
    };
    if soft {
        return Some(guides);
    }
    let centre_point = |guides: &mut Guides| {
        guides.dots.push(Dot {
            centre: frame.corner,
            radius: CENTRE_RADIUS * scale(frame.corner),
            color: PASSIVE,
        });
    };
    let (x, z, dx, dz) = corner_vectors(frame, hit);
    let at = frame.corner;
    match (solid, frame.kind) {
        (Solid::Ball, TargetKind::Sphere) => {
            let grid = (grid > 0.0).then_some(grid);
            guides.lines.extend(lat_lon(&part, at, grid));
        }
        (Solid::Ball, _) if grid > 0.0 => {
            // Round the pole: the two lattice lines through the landing,
            // and the lattice clipped to a disc, `limit` either way.
            let r = ball_radius(&part);
            let q = 0.25 * r;
            let limit = q.max((r * r - (r * r - q * q).sqrt().floor().powi(2)).sqrt());
            let (sx, sz) = (snap_to(dx, grid), snap_to(dz, grid));
            guides.lines.extend(chosen(vec![
                [at + z * sz - x * limit, at + z * sz + x * limit],
                [at + x * sx - z * limit, at + x * sx + z * limit],
            ]));
            centre_point(&mut guides);
            let steps = limit / grid;
            guides.lines.extend(
                Lattice {
                    at,
                    a: x * grid,
                    b: z * grid,
                    min: Vec2::splat(-steps),
                    max: Vec2::splat(steps),
                    radius: Some(steps),
                    exclude: (0, 0),
                }
                .lines(),
            );
        }
        (Solid::Cylinder, TargetKind::Cylinder) => {
            let (r, _) = cylinder_extent(&part);
            let length = frame.size.y;
            let around = |point: Vec3| {
                let x = part.local(point).x;
                chosen(ring(&part, r, |v| Vec3::new(x, v.x, v.y)))
            };
            if grid > 0.0 {
                // A ladder along the side from the near end to the middle,
                // the rung the drag lands on left out, with the ring there.
                let half = 0.5 * length;
                let step = (dz / grid + 0.5).floor();
                guides.lines.extend(
                    Lattice {
                        at,
                        a: x * grid,
                        b: z * grid,
                        min: Vec2::new(-1.0, 0.0),
                        max: Vec2::new(1.0, half / grid),
                        radius: None,
                        exclude: (0, step as i64),
                    }
                    .lines(),
                );
                guides.lines.extend(chosen(vec![[at, at + z * half]]));
                guides.lines.extend(around(at + z * step * grid));
            } else {
                guides.lines.extend(around(at + z * dz));
                guides.lines.extend(chosen(vec![[at, at + z * length]]));
            }
        }
        (Solid::Cylinder, _) if grid > 0.0 => {
            centre_point(&mut guides);
            guides.lines.extend(super::ruler::target(frame, hit, grid));
        }
        _ => {}
    }
    Some(guides)
}

/// `LatLonGuide`: the latitude ring and the quarter meridian through the
/// snapped point `at` on the ball, both over everything; with the grid, the
/// meridians a grid step either side (`grid / r` round, which is the true
/// step only on the equator) and a short arc across the current meridian at
/// every grid latitude on the point's half of the ball.
fn lat_lon(part: &Part, at: Vec3, grid: Option<f32>) -> Vec<Line> {
    let r = ball_radius(part);
    let local = part.local(at);
    let y = local.y;
    let longitude = local.x.atan2(local.z);
    let latitude = (r * r - y * y).max(0.0).sqrt();
    let mut lines = chosen(ring(part, latitude, |v| Vec3::new(v.x, y, v.y)));
    let quarter: Vec<Vec2> = if y >= 0.0 {
        (0..=3)
            .map(|i| Vec2::new(i as f32, 3.0))
            .chain((0..=3).rev().map(|i| Vec2::new(3.0, i as f32)))
            .collect()
    } else {
        (0..=3)
            .map(|i| Vec2::new(-3.0, i as f32))
            .chain((-3..=0).map(|i| Vec2::new(i as f32, 3.0)))
            .collect()
    };
    let meridian = |yaw: f32| {
        let turn = Mat3::from_rotation_y(yaw);
        let points: Vec<Vec3> = quarter
            .iter()
            .map(|v| {
                let v = v.normalize() * r;
                part.world(turn * Vec3::new(0.0, v.x, v.y))
            })
            .collect();
        path(&points, false)
    };
    lines.extend(chosen(meridian(longitude)));
    let Some(grid) = grid else {
        return lines;
    };
    let theta = grid / r;
    lines.extend(major(meridian(longitude + theta)));
    lines.extend(major(meridian(longitude - theta)));
    let turn = Mat3::from_rotation_y(longitude);
    let side = if y >= 0.0 { 1.0 } else { -1.0 };
    let mut ticks = Vec::new();
    // Studio draws every one; one past 1024 of them is thinned as its
    // `Grid3D` thins a lattice [inferred: a guard Studio's own loop lacks].
    let count = ((r + 1e-4) / grid).floor() as i64;
    for step in thinned(1, count) {
        let height = side * step as f32 * grid;
        if (height - y).abs() < 0.01 {
            continue;
        }
        let ring_radius = (r * r - height * height).max(0.0).sqrt();
        let chord = 2.0 * ring_radius * (theta * 0.5).sin();
        let phi = std::f32::consts::PI - theta * 0.5;
        let place = |x: f32, z: f32| part.world(turn * Vec3::new(x, height, z));
        let middle = place(0.0, ring_radius);
        let (across, back) = (chord * phi.cos(), ring_radius - chord * phi.sin());
        ticks.push([place(across, back), middle]);
        ticks.push([middle, place(-across, back)]);
    }
    lines.extend(major(ticks));
    lines
}

#[cfg(test)]
#[path = "round/tests.rs"]
mod tests;
