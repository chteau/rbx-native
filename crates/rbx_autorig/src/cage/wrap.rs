//! The last stage of [`super::fit_cages`]: wrapping the coarse cage onto
//! the body as an even, unfolded shell just outside it.

use glam::Vec3;

use super::closest::Surface;
use super::winding::Solid;
use super::Cage;
use crate::split::Body;

/// Signed distance to the body: negative inside any part, judged
/// by winding number so edges and creases stay right. A cage that sits
/// inside any part would show that part through clothing, so no piece may
/// be exempt.
struct Field {
    surface: Surface,
    solid: Solid,
}

impl Field {
    fn new(body: &Body) -> Self {
        let world = |pm: &crate::PartMesh, t: &[u32; 3]| {
            t.map(|i| pm.centre + pm.mesh.positions[i as usize])
        };
        let solid = Solid::new(
            body.parts
                .iter()
                .map(|pm| pm.mesh.triangles.iter().map(|t| world(pm, t)).collect())
                .collect(),
        );
        // Caps sit inside the body, and so does a shell buried in another (a
        // neck reaching up into the head): measured against either, a point
        // deep under the skin would seem a hair from getting out and never be
        // pushed far enough. A face is outside when a point just off it is.
        let surface = body
            .parts
            .iter()
            .map(|pm| {
                let keep = pm.mesh.triangles.len().saturating_sub(pm.cap_triangles);
                pm.mesh.triangles[..keep]
                    .iter()
                    .map(|t| world(pm, t))
                    .filter(|t| {
                        (t[1] - t[0])
                            .cross(t[2] - t[0])
                            .try_normalize()
                            .is_some_and(|n| !solid.inside((t[0] + t[1] + t[2]) / 3.0 + n * 1e-3))
                    })
                    .collect()
            })
            .collect();
        Self {
            surface: Surface::new(surface),
            solid,
        }
    }

    fn probe(&self, q: Vec3) -> Option<Probe> {
        let (d, at, normal) = self.surface.closest(q)?;
        let inside = self.solid.inside(q);
        let away = if inside { at - q } else { q - at };
        // On the surface itself the way out is the face's own normal.
        let out = if d > 1e-5 { away / d } else { normal };
        Some(Probe {
            sd: if inside { -d } else { d },
            at,
            out,
        })
    }

    fn signed(&self, q: Vec3) -> Option<f32> {
        self.probe(q).map(|p| p.sd)
    }

    /// The move that takes `q` to `margin` outside: along the cage normal
    /// `n` when that gets it at least halfway there, else straight out
    /// through its closest exposed surface point. Deep under the skin a
    /// normal can run along a limb instead of out of it (a joint's buried
    /// rounded end), and following it would drag the point down the limb.
    fn push(&self, q: Vec3, n: Vec3, p: &Probe, margin: f32) -> Vec3 {
        let along = n * (margin - p.sd);
        let enough = p.sd + 0.5 * (margin - p.sd);
        if self.signed(q + along).is_some_and(|s| s >= enough) {
            along
        } else {
            p.at + p.out * margin - q
        }
    }

    /// One iteration's raw move for `q`: out if nearer than `margin`, in by
    /// half the excess over twice the margin if further than three margins
    /// (never past `margin`), else none.
    fn step(&self, q: Vec3, n: Vec3, margin: f32) -> Vec3 {
        let Some(p) = self.probe(q) else {
            return Vec3::ZERO;
        };
        if p.sd < margin {
            self.push(q, n, &p, margin)
        } else if p.sd > 3.0 * margin {
            let d = -n * (0.5 * (p.sd - 2.0 * margin));
            match self.signed(q + d) {
                Some(s) if s >= margin && s < p.sd => d,
                _ => Vec3::ZERO,
            }
        } else {
            Vec3::ZERO
        }
    }

    /// `q`, or if it is not clear of the body, the nearest spot that is,
    /// found by reaching ever further out through the closest surface point
    /// or along either way of the normal. Far enough, every direction
    /// leaves the body, so this always ends outside.
    fn way_out(&self, q: Vec3, n: Vec3, margin: f32) -> Vec3 {
        let clear = margin * 0.25;
        let Some(p) = self.probe(q).filter(|p| p.sd < clear) else {
            return q;
        };
        for k in 0..14 {
            let reach = margin * (1u32 << k) as f32;
            for c in [p.at + p.out * reach, q + n * reach, q - n * reach] {
                if self.signed(c).is_some_and(|s| s >= clear) {
                    return c;
                }
            }
        }
        q
    }
}

/// Where a point stands against the body.
struct Probe {
    /// Signed distance: negative inside.
    sd: f32,
    /// The closest exposed surface point.
    at: Vec3,
    /// The unit direction from `at` away from the body.
    out: Vec3,
}

/// Wraps the coarse cage onto the body as an even shell `margin` outside
/// it. Points travel along the cage's own normals rather than to their
/// closest body point: closest-point targets gather many points into the
/// same fold (a full belly, a low crotch) and tangle the cage.
pub(super) fn shrink_wrap(body: &Body, points: &mut [Vec3], coarse: &[Vec3], cage: &Cage) {
    let field = Field::new(body);
    // A point on no triangle has no normal to travel along.
    let loose: Vec<bool> = cage.neighbours.iter().map(Vec::is_empty).collect();
    // Each point's offset from its neighbours' mean in the coarse cage: the
    // template's own spacing, which relaxation restores.
    let detail: Vec<Vec3> = (0..coarse.len())
        .map(|i| {
            cage.neighbour_mean(i, coarse)
                .map_or(Vec3::ZERO, |mean| coarse[i] - mean)
        })
        .collect();
    let (lo, hi) = crate::mesh::bounds(
        body.parts
            .iter()
            .flat_map(|pm| pm.mesh.positions.iter().map(move |v| pm.centre + *v)),
    );
    let height = if hi.y > lo.y { hi.y - lo.y } else { 5.2 };
    let margin = 0.02 * height / 5.2;
    // Points the field never had to move already sit where the template
    // put them; relaxing them too would drag a well-placed shell along
    // after every pushed neighbour and fold it.
    let mut active = vec![false; points.len()];

    for _ in 0..10 {
        let normals = cage.normals(points);
        let mut step: Vec<Vec3> = (0..points.len())
            .map(|i| {
                if loose[i] {
                    Vec3::ZERO
                } else {
                    field.step(points[i], normals[i], margin)
                }
            })
            .collect();
        let moving: Vec<bool> = step.iter().map(|d| *d != Vec3::ZERO).collect();
        for (a, &m) in active.iter_mut().zip(&moving) {
            *a |= m;
        }
        for _ in 0..3 {
            step = (0..step.len())
                .map(|i| match cage.neighbour_mean(i, &step) {
                    Some(mean) => step[i] * 0.5 + mean * 0.5,
                    None => step[i],
                })
                .collect();
        }
        // Only points that needed a move take one: a point already sitting
        // well in the shell would otherwise be dragged after its pushed
        // neighbours every pass and wander off its template place.
        for ((q, d), &m) in points.iter_mut().zip(&step).zip(&moving) {
            if m {
                *q += *d;
            }
        }
        // Moving only along normals bunches triangles where the body
        // narrows; sliding points within the surface evens them out
        // without undoing the push. They slide toward the template's own
        // spacing rather than the bare mean, which would flatten its finer
        // regions and drift every point off its template place.
        let normals = cage.normals(points);
        for _ in 0..2 {
            let moved: Vec<Vec3> = (0..points.len())
                .map(|i| {
                    let q = points[i];
                    if !active[i] {
                        return q;
                    }
                    let Some(mean) = cage.neighbour_mean(i, points) else {
                        return q;
                    };
                    let d = (mean + detail[i] - q) * 0.3;
                    q + d - normals[i] * d.dot(normals[i])
                })
                .collect();
            points.copy_from_slice(&moved);
        }
    }

    // Untangling can pull a point back under the skin and pushing it out
    // can fold a triangle again, so the two alternate.
    for _ in 0..ROUNDS {
        untangle(points, coarse, cage, &detail);
        push_out(&field, points, cage, &loose, margin);
    }
}

/// Untangle-then-push rounds at the end of [`shrink_wrap`].
const ROUNDS: usize = 4;

/// Smoothing and relaxing can leave a point under the skin; this
/// unsmoothed push brings every point back out. In a crease one push can
/// land under a neighbouring face, so it repeats.
fn push_out(field: &Field, points: &mut [Vec3], cage: &Cage, loose: &[bool], margin: f32) {
    let normals = cage.normals(points);
    for ((q, &n), &skip) in points.iter_mut().zip(&normals).zip(loose) {
        if skip {
            continue;
        }
        for _ in 0..8 {
            match field.probe(*q) {
                Some(p) if p.sd < margin * 0.5 => *q += field.push(*q, n, &p, margin),
                _ => break,
            }
        }
        *q = field.way_out(*q, n, margin);
    }
}

/// Relaxes the neighbourhood of every folded triangle back toward the
/// template's own spacing. Where the body creases (an armpit pressed to the
/// chest, a low crotch) pushing points out along their normals can turn a
/// triangle over; the fold shows through any garment wrapped on it, and
/// only its neighbourhood needs smoothing, not the whole shell.
fn untangle(points: &mut [Vec3], coarse: &[Vec3], cage: &Cage, detail: &[Vec3]) {
    let normal = |field: &[Vec3], t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| field[i as usize]);
        (b - a).cross(c - a)
    };
    for _ in 0..UNTANGLE_PASSES {
        let mut marked = vec![false; points.len()];
        let mut any = false;
        for t in cage.tris.iter().flatten() {
            if normal(points, t).dot(normal(coarse, t)) < 0.0 {
                any = true;
                for &i in t {
                    marked[i as usize] = true;
                    for &j in &cage.neighbours[i as usize] {
                        marked[j as usize] = true;
                    }
                }
            }
        }
        if !any {
            break;
        }
        let moved: Vec<Vec3> = (0..points.len())
            .map(|i| match cage.neighbour_mean(i, points) {
                Some(mean) if marked[i] => points[i] + (mean + detail[i] - points[i]) * 0.5,
                _ => points[i],
            })
            .collect();
        points.copy_from_slice(&moved);
    }
}

/// How many relaxation passes [`untangle`] may take before giving up on a
/// fold.
const UNTANGLE_PASSES: usize = 30;
