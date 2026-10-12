//! Closest-point queries against the body's outer surface.

use glam::Vec3;

/// One surface triangle with what every query needs precomputed.
struct Tri {
    a: Vec3,
    ab: Vec3,
    ac: Vec3,
    normal: Vec3,
}

/// One part's triangles behind their bounding box, so whole parts that
/// cannot beat the current best distance are skipped.
struct Group {
    lo: Vec3,
    hi: Vec3,
    tris: Vec<Tri>,
}

pub(super) struct Surface {
    groups: Vec<Group>,
}

impl Surface {
    /// `groups` holds each part's world-space triangles; degenerate ones
    /// are dropped since the region test divides by their area.
    pub(super) fn new(groups: Vec<Vec<[Vec3; 3]>>) -> Self {
        let groups = groups
            .into_iter()
            .filter_map(|tris| {
                let tris: Vec<Tri> = tris
                    .into_iter()
                    .filter_map(|[a, b, c]| {
                        let ab = b - a;
                        let ac = c - a;
                        let normal = ab.cross(ac).try_normalize()?;
                        Some(Tri { a, ab, ac, normal })
                    })
                    .collect();
                if tris.is_empty() {
                    return None;
                }
                let (lo, hi) =
                    crate::mesh::bounds(tris.iter().flat_map(|t| [t.a, t.a + t.ab, t.a + t.ac]));
                Some(Group { lo, hi, tris })
            })
            .collect();
        Self { groups }
    }

    /// Unsigned distance from `p` to the surface, the closest point on it
    /// and the unit normal of the triangle it lies on; `None` for an empty
    /// surface.
    pub(super) fn closest(&self, p: Vec3) -> Option<(f32, Vec3, Vec3)> {
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for g in &self.groups {
            let outside = (g.lo - p).max(p - g.hi).max(Vec3::ZERO);
            if best.is_some_and(|(d, _, _)| outside.length_squared() >= d) {
                continue;
            }
            for t in &g.tris {
                let q = on_triangle(p, t.a, t.ab, t.ac);
                let d = p.distance_squared(q);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, q, t.normal));
                }
            }
        }
        best.map(|(d, q, n)| (d.sqrt(), q, n))
    }
}

/// Closest point on triangle `a, a+ab, a+ac` (Ericson's region test).
fn on_triangle(p: Vec3, a: Vec3, ab: Vec3, ac: Vec3) -> Vec3 {
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let b = a + ab;
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let c = a + ac;
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}
