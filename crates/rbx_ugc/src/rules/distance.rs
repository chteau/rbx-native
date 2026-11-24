//! Where a point sits against a triangle mesh: its signed distance (to the
//! closest point on the surface, negative behind the closest triangle's
//! front face) and whether the mesh encloses it (a generalized winding
//! number above one half). Measured against Studio 0.748 on Roblox's
//! leather jacket, the farthest cage vertex comes out at Studio's distance
//! to the thousandth, and the winding number reproduces its share of the
//! mesh outside the cage closer than the distance's sign does: at a crease
//! of a closed cage the closest triangle can face the wrong way.

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn length(a: V) -> f32 {
    dot(a, a).sqrt()
}

fn add_scaled(a: V, b: V, s: f32) -> V {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// The closest point to `p` on triangle `abc` (Ericson, Real-Time
/// Collision Detection, 5.1.5).
fn closest_on_triangle(p: V, a: V, b: V, c: V) -> V {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add_scaled(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add_scaled(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return add_scaled(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    add_scaled(add_scaled(a, ab, vb * denom), ac, vc * denom)
}

/// The solid angle `abc` subtends at `p`, over 4 pi (Van Oosterom and
/// Strackee): the triangle's share of `p`'s winding number.
fn winding_share(p: V, a: V, b: V, c: V) -> f32 {
    let (a, b, c) = (sub(a, p), sub(b, p), sub(c, p));
    let (la, lb, lc) = (length(a), length(b), length(c));
    let det = dot(a, cross(b, c));
    let div = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
    det.atan2(div) / (2.0 * std::f32::consts::PI)
}

/// A mesh's triangles as corner positions and front-face normals (counter
/// clockwise, as Roblox draws them), with a bounding sphere each so the
/// closest-point search can skip the far ones.
pub(crate) struct Surface {
    tris: Vec<[V; 3]>,
    normals: Vec<V>,
    spheres: Vec<(V, f32)>,
}

impl Surface {
    /// `to_space` maps the mesh's vertices into the space the queries are
    /// in. Out-of-range triangles are skipped.
    pub(crate) fn new(positions: &[V], triangles: &[[u32; 3]], to_space: impl Fn(V) -> V) -> Self {
        let moved: Vec<V> = positions.iter().map(|&p| to_space(p)).collect();
        let tris: Vec<[V; 3]> = triangles
            .iter()
            .filter_map(|t| {
                let corner = |i: u32| moved.get(i as usize).copied();
                Some([corner(t[0])?, corner(t[1])?, corner(t[2])?])
            })
            .collect();
        let spheres = tris
            .iter()
            .map(|[a, b, c]| {
                let centre = [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0);
                let radius = [a, b, c]
                    .iter()
                    .map(|v| length(sub(**v, centre)))
                    .fold(0.0, f32::max);
                (centre, radius)
            })
            .collect();
        let normals = tris
            .iter()
            .map(|[a, b, c]| cross(sub(*b, *a), sub(*c, *a)))
            .collect();
        Self {
            tris,
            normals,
            spheres,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Whether the mesh wraps around `p`, whichever way it faces.
    pub(crate) fn encloses(&self, p: V) -> bool {
        let winding: f32 = self
            .tris
            .iter()
            .map(|[a, b, c]| winding_share(p, *a, *b, *c))
            .sum();
        winding.abs() > 0.5
    }

    /// Distance from `p` to the surface, negative behind the closest
    /// triangle; the first of several equally close ones decides.
    pub(crate) fn signed_distance(&self, p: V) -> f32 {
        let mut best = f32::MAX;
        let mut side = 0.0;
        for (i, (centre, radius)) in self.spheres.iter().enumerate() {
            if length(sub(p, *centre)) - radius >= best {
                continue;
            }
            let [a, b, c] = self.tris[i];
            let q = closest_on_triangle(p, a, b, c);
            let d = length(sub(p, q));
            if d < best {
                best = d;
                side = dot(sub(p, q), self.normals[i]);
            }
        }
        if side < 0.0 {
            -best
        } else {
            best
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An axis-aligned box from `lo` to `hi`, outward-facing triangles.
    pub(crate) fn cube(lo: f32, hi: f32) -> (Vec<V>, Vec<[u32; 3]>) {
        let positions = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { lo } else { hi },
                    if i & 2 == 0 { lo } else { hi },
                    if i & 4 == 0 { lo } else { hi },
                ]
            })
            .collect();
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        (positions, triangles)
    }

    fn unit_box() -> Surface {
        let (p, t) = cube(-1.0, 1.0);
        Surface::new(&p, &t, |v| v)
    }

    #[test]
    fn a_point_inside_a_box_is_negative() {
        let d = unit_box().signed_distance([0.0, 0.0, 0.5]);
        assert!((d + 0.5).abs() < 1e-5, "{d}");
    }

    #[test]
    fn a_point_outside_a_box_is_its_distance() {
        let s = unit_box();
        assert!((s.signed_distance([0.0, 3.0, 0.0]) - 2.0).abs() < 1e-5);
        let corner = s.signed_distance([2.0, 2.0, 2.0]);
        assert!((corner - 3f32.sqrt()).abs() < 1e-5, "{corner}");
    }

    #[test]
    fn the_front_faces_decide_which_side_is_inside() {
        let (p, t) = cube(-1.0, 1.0);
        let flipped: Vec<[u32; 3]> = t.iter().map(|&[a, b, c]| [a, c, b]).collect();
        let s = Surface::new(&p, &flipped, |v| v);
        assert!(s.signed_distance([0.2, 0.0, 0.0]) > 0.0);
        assert!(s.signed_distance([0.0, 3.0, 0.0]) < 0.0);
        assert!(s.encloses([0.2, 0.0, 0.0]));
    }

    #[test]
    fn a_box_with_one_face_open_still_has_an_inside() {
        let (p, mut t) = cube(-1.0, 1.0);
        t.truncate(10);
        let s = Surface::new(&p, &t, |v| v);
        assert!(s.signed_distance([0.0, 0.0, 0.0]) < 0.0);
        assert!(s.signed_distance([0.0, 0.0, 3.0]) > 0.0);
        // Out past the opening but nearer its axis than its rim, a point
        // is behind the rim's faces, though nothing encloses it.
        assert!(s.signed_distance([3.0, 0.0, 0.0]) < 0.0);
        assert!(s.encloses([0.0, 0.0, 0.0]));
        assert!(!s.encloses([3.0, 0.0, 0.0]));
    }
}
