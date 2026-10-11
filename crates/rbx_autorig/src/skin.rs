//! Smooth skinning on top of the 15 parts (option B of the spec): every
//! part's vertices weighted to part-named bones, so a joint bends the flesh
//! round it instead of hinging two rigid pieces.
//!
//! The weights are Pinocchio's bone heat, solved once on the whole welded
//! source mesh: per bone `j`, `(L + H·A) w_j = H·A·p_j`, with `L` the
//! cotangent Laplacian, `A` the vertex areas, `H_i = 1/d_i²` (`d_i` the
//! distance to the nearest bone vertex `i` "sees") and `p_ij = 1` where `j`
//! is that nearest bone. Pinocchio's ray-cast visibility is replaced by the
//! split's ownership (the bone nearest through the body's volume): a vertex
//! sees its owner, and its owner's parent or children only within the ball
//! round the joint between them that the cut itself may reach. That needs no
//! closed volume and keeps a hand on a thigh off the thigh. Unlike
//! Pinocchio, bone `j`'s heat is also held at 0 wherever `j` is not seen
//! (a Dirichlet condition rather than only a pull towards 0), so a bone's
//! influence ends at the edge of the ball round its joint instead of
//! diffusing across a wide torso. The 15 systems are solved with
//! Jacobi-preconditioned conjugate gradients.
//!
//! Each part's vertices then read the heat weights where they lie on the
//! source surface (barycentric, so the cut's new vertices interpolate the
//! edge they split), keep only {own part, its parent, its children}, keep
//! the four largest, and are quantised to bytes summing to exactly 255.
//!
//! # Bones and bind pose
//!
//! A part's bones are named after parts (the 15 R15 names; nothing is ever
//! weighted to `Root` or `HumanoidRootNode`). Bone `B`'s bind pose, in a
//! part's mesh space (vertices relative to the part's centre, unrotated), is
//! a pure translation to `B`'s joint (the joint whose `Part1` is `B`, so bone
//! `LeftUpperArm` sits at the LeftShoulder joint) minus the part's centre.
//! At run time a vertex `v` moves to `Σ wᵢ · Mᵢ · v` (in the part's space),
//! with `Mᵢ = (part.CFrame⁻¹ · boneJointWorldNowᵢ) · bindᵢ⁻¹` and
//! `boneJointWorldNowᵢ = boneᵢ's part's current CFrame · its own rig
//! attachment's offset` (joint − that part's centre, unrotated). At rest
//! every `Mᵢ` is the identity.
//!
//! In world space this is `Σ wᵢ · boneJointWorldNowᵢ · (v_world − jointᵢ)`,
//! which does not depend on the part: where two parts' vertices coincide and
//! carry the same weights (along every cut), they move together.

use std::collections::HashMap;

use glam::{DVec3, Vec3};

use crate::fit::{segment_distance, Fit};
use crate::mesh::{bounds, weld, Mesh};
use crate::rig::{Joint, Part};
use crate::split::{ownership, plane, reach, segments, Body, PartMesh};
use crate::voxel::Grid;

/// Most influences a vertex may carry: Roblox's limit.
pub const MAX_INFLUENCES: usize = 4;

/// Skin weights for every part of a [`Body`], from [`weights`].
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    /// One per part, in [`Part::ALL`] order (as [`Body::parts`]).
    pub parts: Vec<PartSkin>,
    /// The joint positions the bones were bound at, Roblox space, indexed
    /// by [`Joint`] (the fit's joints).
    pub joints: [Vec3; 15],
}

impl Skin {
    pub fn part(&self, part: Part) -> &PartSkin {
        &self.parts[part.index()]
    }

    /// Where bone `bone` sits in world space: its joint.
    pub fn bone_position(&self, bone: Part) -> Vec3 {
        self.joints[bone.joint().index()]
    }
}

/// One part's skinning, parallel to its [`PartMesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct PartSkin {
    pub part: Part,
    /// The bones this part's mesh lists, by part name: its parent part (if
    /// it has one, which all but `LowerTorso` do), itself, then its children
    /// in [`Part::ALL`] order. Parents come before children, so the list is
    /// also a valid bone hierarchy. At most 5 (`UpperTorso`).
    pub bones: Vec<Part>,
    /// Each bone's bind pose in the part's mesh space: a pure translation to
    /// the bone's joint minus the part's centre (for the part's own bone,
    /// exactly its own rig attachment's offset).
    pub binds: Vec<Vec3>,
    /// Per vertex of the part's mesh: up to four `(slot into bones, weight)`,
    /// heaviest first, weights summing to exactly 255. Unused slots repeat
    /// the first slot's bone with weight 0.
    pub influences: Vec<[(u8, u8); 4]>,
}

impl PartSkin {
    /// The slot of the part's own bone in [`PartSkin::bones`].
    pub fn own(&self) -> usize {
        self.bones
            .iter()
            .position(|&b| b == self.part)
            .expect("a part always lists its own bone")
    }

    /// Vertex `vertex`'s non-zero influences as (bone, weight out of 255).
    pub fn weights_of(&self, vertex: usize) -> impl Iterator<Item = (Part, u8)> + '_ {
        self.influences[vertex]
            .iter()
            .filter(|(_, w)| *w > 0)
            .map(|&(slot, w)| (self.bones[slot as usize], w))
    }

    /// The weight (out of 255) vertex `vertex` gives `bone`.
    pub fn weight(&self, vertex: usize, bone: Part) -> u8 {
        self.weights_of(vertex)
            .filter(|&(b, _)| b == bone)
            .map(|(_, w)| w)
            .sum()
    }
}

/// The bones a part's vertices may be weighted to, in the order
/// [`PartSkin::bones`] lists them.
pub fn bones_of(part: Part) -> Vec<Part> {
    let mut out: Vec<Part> = part.parent().into_iter().collect();
    out.push(part);
    out.extend(Part::ALL.into_iter().filter(|c| c.parent() == Some(part)));
    out
}

/// Skin weights for `body`, the split of `mesh` (the source file, as given
/// to [`crate::split`]) placed by `fit`. Never fails: a part vertex that
/// cannot be matched to the source surface (a cap's centre) copies its
/// nearest matched neighbour, and one with no weight on its part's bones
/// goes wholly to its own bone.
pub fn weights(mesh: &Mesh, fit: &Fit, body: &Body) -> Skin {
    let heat = Heat::solve(mesh, fit);
    let lookup = Lookup::new(&heat.positions, &heat.faces, heat.tolerance);
    Skin {
        parts: body
            .parts
            .iter()
            .map(|piece| skin_part(piece, fit, &heat, &lookup))
            .collect(),
        joints: fit.joints,
    }
}

/// Bone heat over the welded source mesh in Roblox space.
struct Heat {
    positions: Vec<Vec3>,
    faces: Vec<[u32; 3]>,
    /// Per welded vertex, per part (bone), the heat weight in [0, 1].
    weights: Vec<[f32; 15]>,
    /// How far off the surface a part vertex may lie and still read it.
    tolerance: f32,
}

/// Relative residual the conjugate gradients stop at.
const TOLERANCE: f64 = 1e-6;
const MAX_ITERATIONS: usize = 3000;

impl Heat {
    fn solve(mesh: &Mesh, fit: &Fit) -> Heat {
        let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
        let positions: Vec<Vec3> = mesh.positions.iter().map(|&p| fit.to_roblox(p)).collect();
        if triangles.is_empty() {
            return Heat {
                positions: Vec::new(),
                faces: Vec::new(),
                weights: Vec::new(),
                tolerance: 0.,
            };
        }
        // Welded exactly as the split welds, so its cut vertices lie on these
        // triangles.
        let (lo, hi) = bounds(triangles.iter().flatten().map(|&i| positions[i as usize]));
        let h = (hi.y - lo.y).max(f32::MIN_POSITIVE);
        let welded = weld(&positions, 1e-5 * (hi - lo).length());
        let faces: Vec<[u32; 3]> = triangles
            .iter()
            .map(|t| t.map(|i| welded.of[i as usize]))
            .filter(|w| w[0] != w[1] && w[1] != w[2] && w[0] != w[2])
            .collect();
        let pos = welded.positions;
        let n = pos.len();

        let grid = Grid::build(&pos, &faces, 128);
        let owners = ownership(&grid, &pos, fit);
        let segs = segments(fit);
        // The ball round each joint inside which a vertex sees both bones:
        // the one the cut itself may reach, which just holds the limb, and
        // never past the middle of the joint's child bone, so a blend stops
        // half way down a limb even where the limb lies against the body.
        let mut ball = [0f32; 15];
        for joint in Joint::ALL {
            if joint != Joint::Root {
                let (_, normal) = plane(fit, joint);
                let bone = segs
                    .iter()
                    .find(|s| s.0 == joint.part())
                    .map_or(f32::INFINITY, |s| (s.2 - s.1).length());
                ball[joint.index()] = reach(&grid, fit, joint, normal, h).min(0.5 * bone);
            }
        }

        let mut used = vec![false; n];
        for f in &faces {
            for &i in f {
                used[i as usize] = true;
            }
        }
        // H_i and the bones (a bit mask) vertex i is attached to.
        let mut heat = vec![0f64; n];
        let mut target = vec![0u16; n];
        let mut seen = vec![0u16; n];
        for i in (0..n).filter(|&i| used[i]) {
            let p = pos[i];
            let mut dist = [f32::INFINITY; 15];
            for (part, a, b) in &segs {
                let d = segment_distance(p, *a, *b);
                let slot = &mut dist[part.index()];
                *slot = slot.min(d);
            }
            let owner = Part::ALL[owners[i] as usize];
            let mut visible = 0u16;
            let mut nearest = f32::INFINITY;
            for part in Part::ALL {
                let joint = if part == owner {
                    None
                } else if owner.parent() == Some(part) {
                    Some(owner.joint())
                } else if part.parent() == Some(owner) {
                    Some(part.joint())
                } else {
                    continue;
                };
                if let Some(j) = joint {
                    if (p - fit.joint(j)).length() > ball[j.index()] {
                        continue;
                    }
                }
                visible |= 1 << part.index();
                nearest = nearest.min(dist[part.index()]);
            }
            for part in Part::ALL {
                if visible & (1 << part.index()) != 0
                    && dist[part.index()] <= nearest * 1.0001 + 1e-6 * h
                {
                    target[i] |= 1 << part.index();
                }
            }
            seen[i] = visible;
            let d = f64::from(nearest.max(1e-3 * h));
            heat[i] = 1. / (d * d);
        }

        // Cotangent Laplacian (negative cotangents clamped: an obtuse
        // triangle must not make the system indefinite) and lumped areas.
        let mut area = vec![0f64; n];
        let mut edges: HashMap<(u32, u32), f64> = HashMap::with_capacity(faces.len() * 2);
        for f in &faces {
            let [a, b, c] = f.map(|i| pos[i as usize].as_dvec3());
            let twice = (b - a).cross(c - a).length();
            for &i in f {
                area[i as usize] += twice / 6.;
            }
            if twice <= 1e-20 {
                continue;
            }
            for k in 0..3 {
                let (o, i, j) = (f[k], f[(k + 1) % 3], f[(k + 2) % 3]);
                let at = |v: u32| pos[v as usize].as_dvec3();
                let (u, v): (DVec3, DVec3) = (at(i) - at(o), at(j) - at(o));
                let cot = u.dot(v) / u.cross(v).length();
                let w = 0.5 * cot.clamp(0., 1e3);
                if w > 0. {
                    *edges.entry((i.min(j), i.max(j))).or_default() += w;
                }
            }
        }
        let mut rows: Vec<Vec<(u32, f64)>> = vec![Vec::new(); n];
        let mut diag: Vec<f64> = (0..n).map(|i| heat[i] * area[i]).collect();
        for (&(i, j), &w) in &edges {
            rows[i as usize].push((j, -w));
            rows[j as usize].push((i, -w));
            diag[i as usize] += w;
            diag[j as usize] += w;
        }
        for d in diag.iter_mut() {
            // Vertices no face uses: an identity row.
            if d.is_nan() || *d <= 0. {
                *d = 1.;
            }
        }
        let mut start = Vec::with_capacity(n + 1);
        let (mut col, mut val) = (Vec::new(), Vec::new());
        start.push(0);
        for mut row in rows {
            row.sort_unstable_by_key(|e| e.0);
            for (j, w) in row {
                col.push(j);
                val.push(w);
            }
            start.push(col.len());
        }
        let matrix = Csr {
            start,
            col,
            val,
            diag,
        };

        let bones: Vec<usize> = (0..15)
            .filter(|&b| target.iter().any(|t| t & (1 << b) != 0))
            .collect();
        let solve = |b: usize| {
            let mask = 1u16 << b;
            let rhs: Vec<f64> = (0..n)
                .map(|i| {
                    if target[i] & mask != 0 {
                        heat[i] * area[i]
                    } else {
                        0.
                    }
                })
                .collect();
            let mut x: Vec<f64> = (0..n)
                .map(|i| if target[i] & mask != 0 { 1. } else { 0. })
                .collect();
            let free: Vec<bool> = seen.iter().map(|s| s & mask != 0).collect();
            pcg(&matrix, &free, &rhs, &mut x);
            (b, x)
        };
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, bones.len().max(1));
        let solved: Vec<(usize, Vec<f64>)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|t| {
                    let mine: Vec<usize> = bones.iter().copied().skip(t).step_by(threads).collect();
                    let solve = &solve;
                    s.spawn(move || mine.into_iter().map(solve).collect::<Vec<_>>())
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a heat solve panicked"))
                .collect()
        });
        let mut weights = vec![[0f32; 15]; n];
        for (b, x) in solved {
            for (w, v) in weights.iter_mut().zip(x) {
                w[b] = v.clamp(0., 1.) as f32;
            }
        }
        Heat {
            positions: pos,
            faces,
            weights,
            tolerance: 1e-3 * h,
        }
    }
}

/// A symmetric sparse matrix: the diagonal apart, rows of off-diagonals.
struct Csr {
    start: Vec<usize>,
    col: Vec<u32>,
    val: Vec<f64>,
    diag: Vec<f64>,
}

impl Csr {
    fn mul(&self, x: &[f64], y: &mut [f64]) {
        for (i, out) in y.iter_mut().enumerate() {
            let mut s = self.diag[i] * x[i];
            for k in self.start[i]..self.start[i + 1] {
                s += self.val[k] * x[self.col[k] as usize];
            }
            *out = s;
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Jacobi-preconditioned conjugate gradients for `a x = b`, from `x`,
/// with `x` held at 0 wherever `free` is false (the principal submatrix of
/// the free unknowns, still symmetric positive definite).
fn pcg(a: &Csr, free: &[bool], b: &[f64], x: &mut [f64]) {
    let n = b.len();
    for (x, &f) in x.iter_mut().zip(free) {
        if !f {
            *x = 0.;
        }
    }
    let mut r = vec![0.; n];
    a.mul(x, &mut r);
    for i in 0..n {
        r[i] = if free[i] { b[i] - r[i] } else { 0. };
    }
    let goal = TOLERANCE * dot(b, b).sqrt();
    let mut z: Vec<f64> = r.iter().zip(&a.diag).map(|(r, d)| r / d).collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut ap = vec![0.; n];
    for _ in 0..MAX_ITERATIONS {
        if dot(&r, &r).sqrt() <= goal || rz == 0. {
            break;
        }
        a.mul(&p, &mut ap);
        for (ap, &f) in ap.iter_mut().zip(free) {
            if !f {
                *ap = 0.;
            }
        }
        let alpha = rz / dot(&p, &ap);
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
            z[i] = r[i] / a.diag[i];
        }
        let next = dot(&r, &z);
        let beta = next / rz;
        rz = next;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
}

/// The source triangles bucketed on a uniform grid, for finding where a
/// part's vertex lies on the source surface.
struct Lookup<'a> {
    positions: &'a [Vec3],
    faces: &'a [[u32; 3]],
    origin: Vec3,
    cell: f32,
    cells: HashMap<[i32; 3], Vec<u32>>,
    tolerance: f32,
}

impl<'a> Lookup<'a> {
    fn new(positions: &'a [Vec3], faces: &'a [[u32; 3]], tolerance: f32) -> Lookup<'a> {
        let (lo, hi) = bounds(faces.iter().flatten().map(|&i| positions[i as usize]));
        let edges: f32 = faces
            .iter()
            .map(|f| {
                let [a, b, c] = f.map(|i| positions[i as usize]);
                (a - b).length() + (b - c).length() + (c - a).length()
            })
            .sum();
        let mean = edges / (3 * faces.len()).max(1) as f32;
        let cell = (1.5 * mean)
            .max((hi - lo).length() / 512.)
            .max(4. * tolerance)
            .max(1e-6);
        let mut lookup = Lookup {
            positions,
            faces,
            origin: lo,
            cell,
            cells: HashMap::new(),
            tolerance,
        };
        for (k, f) in faces.iter().enumerate() {
            let (a, b) = bounds(f.map(|i| positions[i as usize]));
            let (ka, kb) = (lookup.key(a), lookup.key(b));
            for z in ka[2]..=kb[2] {
                for y in ka[1]..=kb[1] {
                    for x in ka[0]..=kb[0] {
                        lookup.cells.entry([x, y, z]).or_default().push(k as u32);
                    }
                }
            }
        }
        lookup
    }

    fn key(&self, p: Vec3) -> [i32; 3] {
        let q = ((p - self.origin) / self.cell).floor();
        [q.x as i32, q.y as i32, q.z as i32]
    }

    /// The source face `q` lies on (within the tolerance) and `q`'s
    /// barycentric coordinates on it.
    fn locate(&self, q: Vec3) -> Option<(usize, [f32; 3])> {
        let k = self.key(q);
        let mut best: Option<(f32, usize, [f32; 3])> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(list) = self.cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) else {
                        continue;
                    };
                    for &f in list {
                        let t = self.faces[f as usize].map(|i| self.positions[i as usize]);
                        let bary = closest_on_triangle(q, t);
                        let p = t[0] * bary[0] + t[1] * bary[1] + t[2] * bary[2];
                        let d = (p - q).length();
                        if best.is_none_or(|(b, _, _)| d < b) {
                            best = Some((d, f as usize, bary));
                        }
                    }
                }
            }
        }
        best.filter(|(d, _, _)| *d <= self.tolerance)
            .map(|(_, f, bary)| (f, bary))
    }
}

/// Barycentric coordinates of the point of triangle `t` closest to `p`
/// (Ericson's region test).
fn closest_on_triangle(p: Vec3, t: [Vec3; 3]) -> [f32; 3] {
    let [a, b, c] = t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0. && d2 <= 0. {
        return [1., 0., 0.];
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0. && d4 <= d3 {
        return [0., 1., 0.];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        let v = d1 / (d1 - d3);
        return [1. - v, v, 0.];
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0. && d5 <= d6 {
        return [0., 0., 1.];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        let w = d2 / (d2 - d6);
        return [1. - w, 0., w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0. {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0., 1. - w, w];
    }
    let denom = va + vb + vc;
    if denom.is_nan() || denom.abs() <= f32::MIN_POSITIVE {
        return [1., 0., 0.];
    }
    let (v, w) = (vb / denom, vc / denom);
    [1. - v - w, v, w]
}

/// One part's influences.
fn skin_part(piece: &PartMesh, fit: &Fit, heat: &Heat, lookup: &Lookup) -> PartSkin {
    let bones = bones_of(piece.part);
    let binds = bones
        .iter()
        .map(|b| fit.joint(b.joint()) - piece.centre)
        .collect();
    let own = bones
        .iter()
        .position(|&b| b == piece.part)
        .expect("own bone listed");
    let raw: Vec<Option<Vec<f32>>> = piece
        .mesh
        .positions
        .iter()
        .map(|&v| {
            let (f, bary) = lookup.locate(piece.centre + v)?;
            let face = heat.faces[f];
            Some(
                bones
                    .iter()
                    .map(|b| {
                        (0..3)
                            .map(|k| bary[k] * heat.weights[face[k] as usize][b.index()])
                            .sum()
                    })
                    .collect(),
            )
        })
        .collect();
    let positions = &piece.mesh.positions;
    let influences = (0..raw.len())
        .map(|i| {
            let w = raw[i].as_ref().or_else(|| {
                // Off the source surface (a cap's centre): the nearest vertex
                // that is on it.
                (0..raw.len())
                    .filter(|&j| raw[j].is_some())
                    .min_by(|&a, &b| {
                        (positions[a] - positions[i])
                            .length_squared()
                            .total_cmp(&(positions[b] - positions[i]).length_squared())
                    })
                    .and_then(|j| raw[j].as_ref())
            });
            quantise(w.map_or(&[][..], Vec::as_slice), own)
        })
        .collect();
    PartSkin {
        part: piece.part,
        bones,
        binds,
        influences,
    }
}

/// Weights below this share of the vertex's total are dropped before the
/// four largest are kept.
const NEGLIGIBLE: f32 = 0.01;

/// The four largest of `weights` (per bone slot), renormalised and rounded
/// to bytes summing to exactly 255 by largest remainder; all to `own` when
/// there is nothing to go on.
fn quantise(weights: &[f32], own: usize) -> [(u8, u8); 4] {
    let total: f32 = weights.iter().map(|w| w.max(0.)).sum();
    if total.is_nan() || total <= 1e-6 {
        return [
            (own as u8, 255),
            (own as u8, 0),
            (own as u8, 0),
            (own as u8, 0),
        ];
    }
    let mut list: Vec<(usize, f32)> = weights
        .iter()
        .enumerate()
        .map(|(k, &w)| (k, w.max(0.) / total))
        .filter(|&(_, w)| w >= NEGLIGIBLE)
        .collect();
    list.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    list.truncate(MAX_INFLUENCES);
    let sum: f32 = list.iter().map(|e| e.1).sum();
    let scaled: Vec<f32> = list.iter().map(|e| e.1 / sum * 255.).collect();
    let mut bytes: Vec<u32> = scaled.iter().map(|s| s.floor() as u32).collect();
    let short = 255u32.saturating_sub(bytes.iter().sum());
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by(|&a, &b| {
        (scaled[b] - scaled[b].floor())
            .total_cmp(&(scaled[a] - scaled[a].floor()))
            .then(a.cmp(&b))
    });
    for &k in order.iter().take(short as usize) {
        bytes[k] += 1;
    }
    let mut out: Vec<(u8, u8)> = list
        .iter()
        .zip(&bytes)
        .filter(|(_, &b)| b > 0)
        .map(|(e, &b)| (e.0 as u8, b as u8))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let first = out[0].0;
    out.resize(MAX_INFLUENCES, (first, 0));
    [out[0], out[1], out[2], out[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantised_weights_sum_to_255() {
        for w in [
            vec![1., 1., 1.],
            vec![0.2, 0.3, 0.5, 0.0, 0.7],
            vec![0.001, 0.999],
            vec![0., 0.],
            vec![0.25; 5],
        ] {
            let q = quantise(&w, 1);
            assert_eq!(q.iter().map(|e| u32::from(e.1)).sum::<u32>(), 255, "{w:?}");
            assert!(q.windows(2).all(|p| p[0].1 >= p[1].1));
        }
        assert_eq!(quantise(&[0., 0.], 1)[0], (1, 255));
        assert_eq!(
            quantise(&[1., 1., 1.], 0)[..3]
                .iter()
                .map(|e| e.1)
                .sum::<u8>(),
            255
        );
    }

    #[test]
    fn bone_lists_put_parents_first() {
        assert_eq!(
            bones_of(Part::UpperTorso),
            vec![
                Part::LowerTorso,
                Part::UpperTorso,
                Part::Head,
                Part::LeftUpperArm,
                Part::RightUpperArm
            ]
        );
        assert_eq!(
            bones_of(Part::LowerTorso),
            vec![
                Part::LowerTorso,
                Part::UpperTorso,
                Part::LeftUpperLeg,
                Part::RightUpperLeg
            ]
        );
        assert_eq!(
            bones_of(Part::LeftHand),
            vec![Part::LeftLowerArm, Part::LeftHand]
        );
    }
}
