//! Inside test against the body's parts.
//!
//! The sign of one face normal at the closest point lies at edges and in
//! creases, so insideness comes from the generalized winding number
//! instead: it stays correct wherever the point sits relative to the mesh,
//! and a part made of several intersecting shells needs no untangling.

use glam::Vec3;

struct Closed {
    lo: Vec3,
    hi: Vec3,
    tris: Vec<[Vec3; 3]>,
}

pub(super) struct Solid {
    parts: Vec<Closed>,
}

impl Solid {
    /// `parts` holds each part's world-space triangles, caps included.
    pub(super) fn new(parts: Vec<Vec<[Vec3; 3]>>) -> Self {
        let parts = parts
            .into_iter()
            .map(|tris| {
                let (lo, hi) = crate::mesh::bounds(tris.iter().flatten().copied());
                Closed { lo, hi, tris }
            })
            .collect();
        Self { parts }
    }

    /// Whether `p` lies inside any part.
    pub(super) fn inside(&self, p: Vec3) -> bool {
        self.parts.iter().any(|c| c.holds(p))
    }
}

impl Closed {
    fn holds(&self, p: Vec3) -> bool {
        p.cmpge(self.lo).all() && p.cmple(self.hi).all() && winding_number(&self.tris, p) > 0.5
    }
}

/// Sum of the triangles' signed solid angles seen from `p` over 4π
/// (Van Oosterom-Strackee): 1 inside a closed outward-wound mesh, 0 outside.
fn winding_number(tris: &[[Vec3; 3]], p: Vec3) -> f32 {
    let total: f32 = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| v - p);
            let (la, lb, lc) = (a.length(), b.length(), c.length());
            let num = a.dot(b.cross(c));
            let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
            2.0 * num.atan2(den)
        })
        .sum();
    total / (4.0 * std::f32::consts::PI)
}
