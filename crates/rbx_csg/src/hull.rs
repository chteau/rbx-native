//! Convex hulls of point sets by quickhull: start from a tetrahedron of
//! extreme points, then repeatedly push the hull out to the farthest point
//! still outside a face, replacing every face that point sees with a fan
//! from the horizon. Points within a small tolerance of a face count as on
//! it, so a box's hull keeps only its eight corners.

use std::collections::{HashMap, HashSet};

use glam::DVec3;

/// A closed convex hull: its corner points and outward, counter-clockwise
/// triangles over them.
#[derive(Debug, Clone, PartialEq)]
pub struct Hull {
    pub points: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
}

struct Face {
    corners: [usize; 3],
    normal: DVec3,
    offset: f64,
    outside: Vec<usize>,
    alive: bool,
}

impl Face {
    fn new(points: &[DVec3], corners: [usize; 3]) -> Face {
        let [a, b, c] = corners.map(|i| points[i]);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        Face {
            corners,
            normal,
            offset: normal.dot(a),
            outside: Vec::new(),
            alive: true,
        }
    }

    fn distance(&self, point: DVec3) -> f64 {
        self.normal.dot(point) - self.offset
    }
}

impl Hull {
    /// The hull of `points`, `None` when they are fewer than four or lie
    /// (nearly) on one plane.
    pub fn new(points: &[DVec3]) -> Option<Hull> {
        // A solid's corners repeat once per face around them.
        let mut seen_points = HashSet::new();
        let points: Vec<DVec3> = points
            .iter()
            .copied()
            .filter(|p| seen_points.insert(p.to_array().map(f64::to_bits)))
            .collect();
        let points = &points[..];
        let (lo, hi) = points
            .iter()
            .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), &p| {
                (lo.min(p), hi.max(p))
            });
        let eps = (hi - lo).length() * 1e-7;
        let simplex = initial_simplex(points, eps)?;
        let mut faces: Vec<Face> = Vec::new();
        let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
        let inside = simplex.iter().map(|&i| points[i]).sum::<DVec3>() / 4.0;
        for [a, b, c] in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut corners = [simplex[a], simplex[b], simplex[c]];
            if Face::new(points, corners).distance(inside) > 0.0 {
                corners.swap(1, 2);
            }
            add_face(&mut faces, &mut edges, Face::new(points, corners));
        }
        let all: Vec<usize> = (0..points.len()).filter(|i| !simplex.contains(i)).collect();
        assign(points, &mut faces, 0..4, all, eps);

        let mut todo: Vec<usize> = (0..4).collect();
        while let Some(start) = todo.pop() {
            let face = &faces[start];
            if !face.alive || face.outside.is_empty() {
                continue;
            }
            let apex = *face.outside.iter().max_by(|&&a, &&b| {
                face.distance(points[a])
                    .total_cmp(&face.distance(points[b]))
            })?;
            // Every face the apex sees, flooding out from the first one.
            let mut visible = vec![start];
            let mut seen = HashSet::from([start]);
            let mut i = 0;
            while i < visible.len() {
                let corners = faces[visible[i]].corners;
                for k in 0..3 {
                    let twin = (corners[(k + 1) % 3], corners[k]);
                    let &next = edges.get(&twin)?;
                    if seen.insert(next) && faces[next].distance(points[apex]) > eps {
                        visible.push(next);
                    }
                }
                i += 1;
            }
            let lit: HashSet<usize> = visible.iter().copied().collect();
            let mut horizon = Vec::new();
            let mut orphans = Vec::new();
            for &v in &visible {
                let corners = faces[v].corners;
                for k in 0..3 {
                    let edge = (corners[k], corners[(k + 1) % 3]);
                    if !lit.contains(edges.get(&(edge.1, edge.0))?) {
                        horizon.push(edge);
                    }
                }
                orphans.append(&mut faces[v].outside);
            }
            for &v in &visible {
                faces[v].alive = false;
                let corners = faces[v].corners;
                for k in 0..3 {
                    edges.remove(&(corners[k], corners[(k + 1) % 3]));
                }
            }
            let first = faces.len();
            for (a, b) in horizon {
                add_face(&mut faces, &mut edges, Face::new(points, [a, b, apex]));
            }
            orphans.retain(|&p| p != apex);
            let end = faces.len();
            assign(points, &mut faces, first..end, orphans, eps);
            todo.extend(first..end);
        }

        let mut remap = vec![u32::MAX; points.len()];
        let mut hull = Hull {
            points: Vec::new(),
            triangles: Vec::new(),
        };
        for face in faces.iter().filter(|f| f.alive) {
            let triangle = face.corners.map(|i| {
                if remap[i] == u32::MAX {
                    remap[i] = hull.points.len() as u32;
                    hull.points.push(points[i]);
                }
                remap[i]
            });
            hull.triangles.push(triangle);
        }
        Some(hull)
    }

    /// The centre of the enclosed volume.
    pub fn centroid(&self) -> DVec3 {
        let (volume, moment) = self.triangles.iter().fold((0.0, DVec3::ZERO), |(v, m), t| {
            let [a, b, c] = t.map(|i| self.points[i as usize]);
            let dv = a.dot(b.cross(c)) / 6.0;
            (v + dv, m + (a + b + c) / 4.0 * dv)
        });
        moment / volume
    }

    /// The enclosed volume.
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.points[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

fn add_face(faces: &mut Vec<Face>, edges: &mut HashMap<(usize, usize), usize>, face: Face) {
    let c = face.corners;
    for k in 0..3 {
        edges.insert((c[k], c[(k + 1) % 3]), faces.len());
    }
    faces.push(face);
}

/// Hands each of `candidates` to the first face in `range` it lies outside
/// of; a point inside them all is inside the hull for good.
fn assign(
    points: &[DVec3],
    faces: &mut [Face],
    range: std::ops::Range<usize>,
    candidates: Vec<usize>,
    eps: f64,
) {
    for p in candidates {
        if let Some(face) = faces[range.clone()]
            .iter_mut()
            .find(|f| f.distance(points[p]) > eps)
        {
            face.outside.push(p);
        }
    }
}

/// Four points spanning a tetrahedron as large as the extremes allow.
fn initial_simplex(points: &[DVec3], eps: f64) -> Option<[usize; 4]> {
    let extremes: Vec<usize> = (0..3)
        .flat_map(|axis| {
            let by = |a: &&DVec3, b: &&DVec3| a[axis].total_cmp(&b[axis]);
            let enumerate = || points.iter().enumerate();
            [
                enumerate().min_by(|a, b| by(&a.1, &b.1)).map(|(i, _)| i),
                enumerate().max_by(|a, b| by(&a.1, &b.1)).map(|(i, _)| i),
            ]
        })
        .collect::<Option<_>>()?;
    let (mut a, mut b) = (extremes[0], extremes[1]);
    for &i in &extremes {
        for &j in &extremes {
            if points[i].distance_squared(points[j]) > points[a].distance_squared(points[b]) {
                (a, b) = (i, j);
            }
        }
    }
    let axis = (points[b] - points[a]).normalize_or_zero();
    let off_line = |i: &usize| {
        let d = points[*i] - points[a];
        (d - axis * d.dot(axis)).length()
    };
    let c = (0..points.len()).max_by(|x, y| off_line(x).total_cmp(&off_line(y)))?;
    if off_line(&c) <= eps {
        return None;
    }
    let normal = (points[b] - points[a])
        .cross(points[c] - points[a])
        .normalize();
    let off_plane = |i: &usize| normal.dot(points[*i] - points[a]).abs();
    let d = (0..points.len()).max_by(|x, y| off_plane(x).total_cmp(&off_plane(y)))?;
    (off_plane(&d) > eps).then_some([a, b, c, d])
}

#[cfg(test)]
mod tests;
