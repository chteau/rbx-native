//! Bringing a part under its triangle budget: quadric-error edge collapses
//! (Garland and Heckbert) on the part's surface, never on its caps.
//!
//! What must not move, and why:
//! - The caps and the cut loops they close. A neighbouring part is capped
//!   with the same loop, so the two parts still meet exactly.
//! - Any other open or non-manifold edge, where the surface has no fan to
//!   collapse into.
//! - Vertices on a UV or normal seam (more than one render vertex at one
//!   position). Moving one would pull a texture island across the seam.
//!
//! Every collapse is a half-edge collapse: the removed vertex goes to an
//! existing one, so positions, normals and UVs are all the source's and skin
//! weights computed afterwards follow the kept vertices. Ties are broken by
//! vertex index, so the same input always gives the same output.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DVec3, Vec3};

use crate::mesh::bounds;
use crate::split::PartMesh;

/// A symmetric 4×4 error quadric, upper triangle row by row.
#[derive(Debug, Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    /// Squared distance to the plane `n·p + d = 0`, times `w`.
    fn plane(n: DVec3, d: f64, w: f64) -> Self {
        let (a, b, c) = (n.x, n.y, n.z);
        Quadric(
            [
                a * a,
                a * b,
                a * c,
                a * d,
                b * b,
                b * c,
                b * d,
                c * c,
                c * d,
                d * d,
            ]
            .map(|q| q * w),
        )
    }

    fn add(&mut self, other: &Quadric) {
        for (q, o) in self.0.iter_mut().zip(other.0) {
            *q += o;
        }
    }

    fn sum(&self, other: &Quadric) -> Quadric {
        let mut q = *self;
        q.add(other);
        q
    }

    fn error(&self, p: DVec3) -> f64 {
        let q = &self.0;
        let (x, y, z) = (p.x, p.y, p.z);
        q[0] * x * x
            + 2. * q[1] * x * y
            + 2. * q[2] * x * z
            + 2. * q[3] * x
            + q[4] * y * y
            + 2. * q[5] * y * z
            + 2. * q[6] * y
            + q[7] * z * z
            + 2. * q[8] * z
            + q[9]
    }
}

/// Collapsing welded vertex `v` into its neighbour `u`, at `cost`. The
/// versions are the two vertices' when it was queued; a later collapse
/// around either makes the entry stale.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    cost: f64,
    v: u32,
    u: u32,
    versions: (u32, u32),
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    /// Reversed, so `BinaryHeap` pops the cheapest (then lowest-indexed)
    /// collapse first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.v.cmp(&self.v))
            .then(other.u.cmp(&self.u))
    }
}

/// The smallest cosine allowed between a triangle's normal before and after
/// a collapse: no fold-overs, and no triangle swung nearly edge-on.
const MIN_TURN_COS: f64 = 0.2;

/// How many of `piece`'s triangles are surface rather than cap.
pub(crate) fn surface_triangles(piece: &PartMesh) -> usize {
    piece.mesh.triangles.len() - piece.cap_triangles
}

/// Collapses edges of `piece`'s surface until at most `target` surface
/// triangles remain or no collapse is allowed, then drops the unused
/// vertices and recentres the part on its (possibly smaller) bounds.
pub(crate) fn simplify(piece: &mut PartMesh, target: usize) {
    let surface = surface_triangles(piece);
    if surface <= target {
        return;
    }
    let mesh = &piece.mesh;

    // Weld render vertices by exact position: a cut loop's surface and cap
    // vertices are copies of one point, as are the two sides of a seam.
    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
    let mut pos: Vec<DVec3> = Vec::new();
    let of: Vec<u32> = mesh
        .positions
        .iter()
        .map(|p| {
            *ids.entry(p.to_array().map(f32::to_bits))
                .or_insert_with(|| {
                    pos.push(p.as_dvec3());
                    (pos.len() - 1) as u32
                })
        })
        .collect();
    let n = pos.len();

    let mut faces: Vec<[u32; 3]> = mesh.triangles[..surface].to_vec();
    let mut alive = vec![true; surface];
    let welded = |f: &[u32; 3]| f.map(|r| of[r as usize]);

    // The caps' own edges count for the link condition: a collapse that
    // made a surface edge out of a cap's diagonal would use it four times.
    let mut locked = vec![false; n];
    let mut cap_links: Vec<Vec<u32>> = vec![Vec::new(); n];
    for t in &mesh.triangles[surface..] {
        let w = t.map(|r| of[r as usize]);
        for k in 0..3 {
            locked[w[k] as usize] = true;
            cap_links[w[k] as usize].extend([w[(k + 1) % 3], w[(k + 2) % 3]]);
        }
    }
    let mut edge_use: HashMap<(u32, u32), u32> = HashMap::new();
    for f in &faces {
        let w = welded(f);
        for k in 0..3 {
            let (a, b) = (w[k], w[(k + 1) % 3]);
            *edge_use.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    for (&(a, b), &uses) in &edge_use {
        if uses != 2 {
            locked[a as usize] = true;
            locked[b as usize] = true;
        }
    }
    // One render vertex per welded vertex, or it lies on a seam.
    let mut render: Vec<Option<u32>> = vec![None; n];
    for f in &faces {
        for &r in f {
            let w = of[r as usize] as usize;
            match render[w] {
                None => render[w] = Some(r),
                Some(seen) if seen != r => locked[w] = true,
                Some(_) => {}
            }
        }
    }

    let mut incident: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut quadric = vec![Quadric::default(); n];
    for (i, f) in faces.iter().enumerate() {
        let w = welded(f);
        for &v in &w {
            incident[v as usize].push(i as u32);
        }
        let [a, b, c] = w.map(|v| pos[v as usize]);
        let cross = (b - a).cross(c - a);
        let area = cross.length() / 2.;
        if let Some(normal) = cross.try_normalize() {
            let q = Quadric::plane(normal, -normal.dot(a), area);
            for &v in &w {
                quadric[v as usize].add(&q);
            }
        }
    }

    let mut version = vec![0u32; n];
    let mut heap = BinaryHeap::new();
    let queue =
        |heap: &mut BinaryHeap<Candidate>, quadric: &[Quadric], version: &[u32], v: u32, u: u32| {
            let cost = quadric[v as usize]
                .sum(&quadric[u as usize])
                .error(pos[u as usize]);
            heap.push(Candidate {
                cost,
                v,
                u,
                versions: (version[v as usize], version[u as usize]),
            });
        };
    for f in &faces {
        let w = welded(f);
        for k in 0..3 {
            let (a, b) = (w[k], w[(k + 1) % 3]);
            if !locked[a as usize] {
                queue(&mut heap, &quadric, &version, a, b);
            }
            if !locked[b as usize] {
                queue(&mut heap, &quadric, &version, b, a);
            }
        }
    }

    let mut remaining = surface;
    while remaining > target {
        let Some(c) = heap.pop() else { break };
        let (v, u) = (c.v as usize, c.u as usize);
        if c.versions != (version[v], version[u]) || incident[v].is_empty() {
            continue;
        }
        let Some(ru) = collapsible(&faces, &incident, &cap_links, &of, &pos, v, u) else {
            continue;
        };
        for &f in &incident[v].clone() {
            let w = welded(&faces[f as usize]);
            if w.contains(&(u as u32)) {
                alive[f as usize] = false;
                for &x in &w {
                    if x as usize != v {
                        incident[x as usize].retain(|&g| g != f);
                    }
                }
                remaining -= 1;
            } else {
                let k = w.iter().position(|&x| x as usize == v).expect("incident");
                faces[f as usize][k] = ru;
                incident[u].push(f);
            }
        }
        incident[v].clear();
        let qv = quadric[v];
        quadric[u].add(&qv);
        version[v] += 1;
        version[u] += 1;
        let mut around: Vec<u32> = incident[u]
            .iter()
            .flat_map(|&f| welded(&faces[f as usize]))
            .filter(|&x| x as usize != u)
            .collect();
        around.sort_unstable();
        around.dedup();
        for w in around {
            if !locked[u] {
                queue(&mut heap, &quadric, &version, u as u32, w);
            }
            if !locked[w as usize] {
                queue(&mut heap, &quadric, &version, w, u as u32);
            }
        }
    }

    // Keep the surviving surface, then the caps, and only the vertices they
    // use, in their original order.
    let mut triangles: Vec<[u32; 3]> = faces
        .iter()
        .zip(&alive)
        .filter(|(_, &a)| a)
        .map(|(f, _)| *f)
        .collect();
    triangles.extend_from_slice(&mesh.triangles[surface..]);
    let mut used = vec![false; mesh.positions.len()];
    for t in &triangles {
        for &r in t {
            used[r as usize] = true;
        }
    }
    let mut renumber = vec![u32::MAX; used.len()];
    let mut kept = 0u32;
    for (r, &u) in used.iter().enumerate() {
        if u {
            renumber[r] = kept;
            kept += 1;
        }
    }
    let keep = |r: usize| used[r];
    let positions: Vec<Vec3> = (0..used.len())
        .filter(|&r| keep(r))
        .map(|r| mesh.positions[r])
        .collect();
    let normals: Vec<_> = if mesh.normals.len() == used.len() {
        (0..used.len())
            .filter(|&r| keep(r))
            .map(|r| mesh.normals[r])
            .collect()
    } else {
        Vec::new()
    };
    let uvs: Vec<_> = if mesh.uvs.len() == used.len() {
        (0..used.len())
            .filter(|&r| keep(r))
            .map(|r| mesh.uvs[r])
            .collect()
    } else {
        Vec::new()
    };
    let triangles = triangles
        .into_iter()
        .map(|t| t.map(|r| renumber[r as usize]))
        .collect();

    let (lo, hi) = bounds(positions.iter().copied());
    let shift = (lo + hi) / 2.;
    piece.mesh.positions = positions.into_iter().map(|p| p - shift).collect();
    piece.mesh.normals = normals;
    piece.mesh.uvs = uvs;
    piece.mesh.triangles = triangles;
    piece.centre += shift;
    piece.size = hi - lo;
    for (_, offset) in piece
        .attachments
        .iter_mut()
        .chain(piece.accessories.iter_mut())
    {
        *offset -= shift;
    }
}

/// Whether welded vertex `v` may collapse into its neighbour `u`, and if so
/// the render vertex of `u` that `v`'s triangles take over.
fn collapsible(
    faces: &[[u32; 3]],
    incident: &[Vec<u32>],
    cap_links: &[Vec<u32>],
    of: &[u32],
    pos: &[DVec3],
    v: usize,
    u: usize,
) -> Option<u32> {
    let welded = |f: u32| faces[f as usize].map(|r| of[r as usize]);
    let (u32_u, u32_v) = (u as u32, v as u32);
    // The two triangles on the edge: a closed fan round `v` has exactly two.
    let shared: Vec<u32> = incident[v]
        .iter()
        .copied()
        .filter(|&f| welded(f).contains(&u32_u))
        .collect();
    let [f0, f1] = shared[..] else { return None };
    // Both on the same side of any seam through `u`.
    let render_of = |f: u32| {
        let w = welded(f);
        faces[f as usize][w.iter().position(|&x| x == u32_u).expect("shared")]
    };
    let ru = render_of(f0);
    if render_of(f1) != ru {
        return None;
    }
    // Link condition: `u` and `v` share no neighbour (through the surface
    // or a cap) but the two opposite corners, or the collapse would pinch
    // the surface.
    let neighbours = |x: usize| {
        let mut n: Vec<u32> = incident[x]
            .iter()
            .flat_map(|&f| welded(f))
            .chain(cap_links[x].iter().copied())
            .filter(|&y| y as usize != x)
            .collect();
        n.sort_unstable();
        n.dedup();
        n
    };
    let (nv, nu) = (neighbours(v), neighbours(u));
    let common = nv.iter().filter(|x| nu.binary_search(x).is_ok()).count();
    if common != 2 {
        return None;
    }
    // No triangle may flip, nearly flip, or duplicate one `u` already has.
    let mut around_u: Vec<[u32; 3]> = incident[u]
        .iter()
        .map(|&f| {
            let mut w = welded(f);
            w.sort_unstable();
            w
        })
        .collect();
    around_u.sort_unstable();
    for &f in &incident[v] {
        if f == f0 || f == f1 {
            continue;
        }
        let w = welded(f);
        let before = w.map(|x| pos[x as usize]);
        let after = w.map(|x| pos[if x == u32_v { u } else { x as usize }]);
        let n0 = (before[1] - before[0]).cross(before[2] - before[0]);
        let n1 = (after[1] - after[0]).cross(after[2] - after[0]);
        let (Some(n0), Some(n1)) = (n0.try_normalize(), n1.try_normalize()) else {
            return None;
        };
        if n0.dot(n1) < MIN_TURN_COS {
            return None;
        }
        let mut moved = w.map(|x| if x == u32_v { u32_u } else { x });
        moved.sort_unstable();
        if around_u.binary_search(&moved).is_ok() {
            return None;
        }
    }
    Some(ru)
}
