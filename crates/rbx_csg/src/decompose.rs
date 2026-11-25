//! Approximate convex decomposition: the convex pieces a union collides as.
//! Every piece is the solid inside an axis-aligned cell. The piece whose
//! hull overshoots it most has its cell cut in two, again and again, until
//! every piece is within a share of the whole solid's volume of its hull,
//! or the piece budget runs out. A cut takes the plane, among the piece's
//! own corner coordinates, that leaves the least hull volume on its two
//! sides; each new piece is carved from the whole solid with the CSG
//! kernel, so it is a closed solid however many cuts made its cell.

use glam::{DMat4, DVec3};

use crate::bsp::{Plane, Polygon};
use crate::csgphs::points;
use crate::hull::Hull;
use crate::{shapes, Solid};

/// Planes tried along each axis per cut.
const CANDIDATES: usize = 9;

struct Piece {
    cell: (DVec3, DVec3),
    solid: Solid,
    hull: Hull,
    /// Hull volume the piece doesn't fill; `None` once it can't be cut.
    excess: Option<f64>,
}

impl Piece {
    /// `whole` inside `cell`, `None` when that is empty, flat or not a
    /// sound solid (more volume than its own hull).
    fn new(whole: &Solid, cell: (DVec3, DVec3)) -> Option<Piece> {
        let (min, max) = cell;
        let block = shapes::block();
        let model = DMat4::from_translation((min + max) / 2.0) * DMat4::from_scale(max - min);
        let solid = whole
            .clone()
            .intersect(Solid::from_triangles(
                &block.positions,
                &block.indices,
                model,
            ))
            .ok()?;
        let hull = Hull::new(&points(&solid))?;
        let (volume, hull_volume) = (solid.volume(), hull.volume());
        if volume <= 1e-9 || volume > hull_volume * (1.0 + 1e-6) {
            return None;
        }
        Some(Piece {
            cell,
            solid,
            excess: Some(hull_volume - volume),
            hull,
        })
    }
}

/// `solid`'s convex pieces: cut until each overshoots by at most
/// `tolerance` of the solid's volume, into at most `budget` pieces. A flat
/// or empty solid has none.
pub fn decompose(solid: &Solid, tolerance: f64, budget: usize) -> Vec<Hull> {
    let solid = &triangulated(solid);
    let Some((lo, hi)) = solid.bounds() else {
        return Vec::new();
    };
    let pad = DVec3::splat((hi - lo).max_element().max(1.0));
    let Some(whole) = Piece::new(solid, (lo - pad, hi + pad)) else {
        return Hull::new(&points(solid)).into_iter().collect();
    };
    let limit = solid.volume().abs() * tolerance;
    let mut pieces = vec![whole];
    while pieces.len() < budget {
        let Some((worst, _)) = pieces
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i, p.excess?)))
            .filter(|&(_, e)| e > limit)
            .max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            break;
        };
        match cut(solid, &pieces[worst]) {
            Some((a, b)) => {
                pieces.swap_remove(worst);
                pieces.extend([a, b]);
            }
            None => pieces[worst].excess = None,
        }
    }
    pieces.into_iter().map(|p| p.hull).collect()
}

/// `solid` as triangles, each on its own exact plane: a finished boolean's
/// welded polygons are no longer quite flat, and the BSP tree never stops
/// splitting a polygon that lies off its own plane.
fn triangulated(solid: &Solid) -> Solid {
    let polygons = solid
        .polygons
        .iter()
        .flat_map(|polygon| {
            let first = polygon.vertices[0];
            polygon.vertices[1..].windows(2).filter_map(move |pair| {
                Some(Polygon {
                    vertices: vec![first, pair[0], pair[1]],
                    plane: Plane::from_points(first, pair[0], pair[1])?,
                    color: polygon.color,
                })
            })
        })
        .collect();
    Solid { polygons }
}

/// The piece in two at the best axis plane that carves cleanly, `None`
/// when none helps. When no single cut shrinks the hulls (a hole through
/// the middle: each half's hull still spans it), the cut goes through the
/// middle of the volume the hull overshoots by, across the piece's longest
/// side.
fn cut(whole: &Solid, piece: &Piece) -> Option<(Piece, Piece)> {
    let solid = &piece.solid;
    let (lo, hi) = solid.bounds()?;
    let corners = points(solid);
    let hull_volume = piece.hull.volume();
    let mut planes = Vec::new();
    for axis in 0..3 {
        let margin = (hi[axis] - lo[axis]) * 1e-3;
        let mut values: Vec<f64> = corners
            .iter()
            .map(|p| p[axis])
            .filter(|&v| v > lo[axis] + margin && v < hi[axis] - margin)
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| (*a - *b).abs() <= margin);
        let step = values.len().div_ceil(CANDIDATES).max(1);
        let mut tried: Vec<f64> = values.into_iter().step_by(step).collect();
        tried.push((lo[axis] + hi[axis]) / 2.0);
        for value in tried {
            let score = side_hull(solid, axis, value, false) + side_hull(solid, axis, value, true);
            if score < hull_volume * (1.0 - 1e-3) {
                planes.push((score, axis, value));
            }
        }
    }
    planes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut planes: Vec<(usize, f64)> = planes.into_iter().map(|(_, a, v)| (a, v)).collect();
    let missing = hull_volume - solid.volume();
    if let Some(centre) = solid.centroid() {
        let middle = (piece.hull.centroid() * hull_volume - centre * solid.volume()) / missing;
        let axis = (hi - lo).max_position();
        let margin = (hi[axis] - lo[axis]) * 0.05;
        if middle[axis] > lo[axis] + margin && middle[axis] < hi[axis] - margin {
            planes.push((axis, middle[axis]));
        }
    }
    planes.into_iter().find_map(|(axis, value)| {
        let (min, max) = piece.cell;
        let (mut below, mut above) = ((min, max), (min, max));
        below.1[axis] = value;
        above.0[axis] = value;
        // Carving the piece is cheaper; the whole solid is the fallback
        // when a chain of cuts left the piece unsound.
        let carve = |cell| Piece::new(solid, cell).or_else(|| Piece::new(whole, cell));
        Some((carve(below)?, carve(above)?))
    })
}

/// The hull volume of `solid` on one side of the plane `axis = value`: the
/// corners of its faces that reach that side (a face lying in the plane
/// when the solid is on that side of it), and where its edges cross.
fn side_hull(solid: &Solid, axis: usize, value: f64, above: bool) -> f64 {
    let sign = if above { 1.0 } else { -1.0 };
    let side = |p: DVec3| (p[axis] - value) * sign;
    let mut kept = Vec::new();
    for polygon in &solid.polygons {
        let reaches = polygon.vertices.iter().any(|&p| side(p) > 0.0)
            || (polygon.vertices.iter().all(|&p| side(p) == 0.0)
                && polygon.plane.normal[axis] * sign < 0.0);
        if !reaches {
            continue;
        }
        let n = polygon.vertices.len();
        for i in 0..n {
            let (p, q) = (polygon.vertices[i], polygon.vertices[(i + 1) % n]);
            if side(p) >= 0.0 {
                kept.push(p);
            }
            if side(p) * side(q) < 0.0 {
                kept.push(p.lerp(q, (value - p[axis]) / (q[axis] - p[axis])));
            }
        }
    }
    Hull::new(&kept).map_or(0.0, |h| h.volume())
}

#[cfg(test)]
mod tests;
