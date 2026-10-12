//! The triangle mesh the rigger reads and writes, and its welded topology.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::orient::Axis;

/// A triangle mesh of render vertices.
///
/// `normals` and `uvs` are either empty or one per position. UVs use a
/// top-left origin (`v` grows downward), the convention of glTF and of
/// Roblox's own mesh files; the readers flip OBJ and FBX UVs into it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub triangles: Vec<[u32; 3]>,
    /// The up axis the file declares, if its format declares one (glTF is
    /// always +Y; FBX names it in `GlobalSettings`). Orientation detection
    /// prefers it whenever the shape itself does not clearly disagree.
    pub up_hint: Option<Axis>,
    /// The front axis the file declares, as `up_hint`.
    pub front_hint: Option<Axis>,
    /// What the reader changed and the user should know, such as debris it
    /// dropped. Fitting passes these on as warnings.
    pub notes: Vec<String>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Appends `other`, keeping normals and UVs only while every piece has
    /// them (missing UVs become zero once any piece has some).
    pub(crate) fn append(&mut self, other: Mesh) {
        let had = self.positions.len();
        let base = had as u32;
        let normals =
            (had == 0 || self.normals.len() == had) && other.normals.len() == other.positions.len();
        if normals {
            self.normals.extend(other.normals);
        } else {
            self.normals.clear();
        }
        if !other.uvs.is_empty() || !self.uvs.is_empty() {
            self.uvs.resize(had, Vec2::ZERO);
            self.uvs.extend(other.uvs);
            self.uvs.resize(had + other.positions.len(), Vec2::ZERO);
        }
        self.positions.extend(other.positions);
        self.triangles.extend(
            other
                .triangles
                .into_iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }

    /// Drops triangles that address missing vertices or non-finite
    /// positions, so later stages can index and measure freely.
    pub(crate) fn valid_triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        let n = self.positions.len() as u32;
        self.triangles.iter().copied().filter(move |t| {
            t.iter()
                .all(|&i| i < n && self.positions[i as usize].is_finite())
        })
    }
}

/// Render vertices merged by position, so UV and normal seams do not split
/// the surface: what cutting, capping and the watertightness of the output
/// are defined on.
pub(crate) struct Welded {
    pub positions: Vec<Vec3>,
    /// Render vertex → welded vertex.
    pub of: Vec<u32>,
}

/// Merges vertices closer than `epsilon` (a grid hash with the neighbouring
/// cells checked, so a pair straddling a cell border still merges).
pub(crate) fn weld(positions: &[Vec3], epsilon: f32) -> Welded {
    let epsilon = epsilon.max(f32::MIN_POSITIVE);
    let key = |p: Vec3| {
        let q = (p / epsilon).floor();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut out: Vec<Vec3> = Vec::new();
    let mut of = Vec::with_capacity(positions.len());
    for &p in positions {
        let (x, y, z) = key(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = cells.get(&(x + dx, y + dy, z + dz)) {
                        for &w in list {
                            if out[w as usize] == p
                                || (out[w as usize] - p).length_squared() <= epsilon * epsilon
                            {
                                found = Some(w);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let w = found.unwrap_or_else(|| {
            let w = out.len() as u32;
            out.push(p);
            cells.entry((x, y, z)).or_default().push(w);
            w
        });
        of.push(w);
    }
    Welded { positions: out, of }
}

/// Area-weighted vertex normals over `triangles`.
pub(crate) fn smooth_normals(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for t in triangles {
        let [a, b, c] = t.map(|i| positions[i as usize]);
        let n = (b - a).cross(c - a);
        for &i in t {
            normals[i as usize] += n;
        }
    }
    normals
        .into_iter()
        .map(|n| n.try_normalize().unwrap_or(Vec3::Y))
        .collect()
}

/// Axis-aligned bounds of `points`.
pub(crate) fn bounds(points: impl IntoIterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.into_iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

/// The triangles of each connected piece of `mesh` (vertices welded by
/// position first, so UV seams do not split a piece).
pub(crate) fn pieces(mesh: &Mesh) -> Vec<Vec<usize>> {
    let (lo, hi) = bounds(mesh.positions.iter().copied());
    let welded = weld(&mesh.positions, 1e-5 * (hi - lo).length());
    let mut parent: Vec<u32> = (0..welded.positions.len() as u32).collect();
    fn root(parent: &mut [u32], mut v: u32) -> u32 {
        while parent[v as usize] != v {
            parent[v as usize] = parent[parent[v as usize] as usize];
            v = parent[v as usize];
        }
        v
    }
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| welded.of[i as usize]);
        for other in [b, c] {
            let (x, y) = (root(&mut parent, a), root(&mut parent, other));
            if x != y {
                parent[x as usize] = y;
            }
        }
    }
    let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        let r = root(&mut parent, welded.of[t[0] as usize]);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

/// What [`drop_debris`] removed, in triangles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dropped {
    /// Pieces standing apart from the body, such as a prop on the floor.
    pub detached: usize,
    /// Flat pieces under the feet, such as a shadow disc or ground plane.
    pub bases: usize,
}

/// Drops what game characters often ship standing beside or on, which is
/// not body and, voxelised, would join the feet or stretch a limb.
///
/// First every piece farther than 2% of the model's size from the piece
/// holding the most triangles (counting pieces near pieces that are near it:
/// eyes, hair and separate boxy limbs all touch), then any flat piece
/// (thinner than 0.5% of the model along an axis) lying at the lowest or
/// highest point of the rest along that axis and at least 20% as wide as
/// the rest on another axis. The up axis is not known yet, so every axis
/// counts as a floor. Each kind dropped adds a line to `mesh.notes`.
pub(crate) fn drop_debris(mesh: &mut Mesh) -> Dropped {
    let pieces = pieces(mesh);
    let mut dropped = Dropped::default();
    if pieces.len() < 2 {
        return dropped;
    }
    let piece_bounds: Vec<(Vec3, Vec3)> = pieces
        .iter()
        .map(|piece| {
            bounds(
                piece
                    .iter()
                    .flat_map(|&t| mesh.triangles[t])
                    .map(|i| mesh.positions[i as usize]),
            )
        })
        .collect();
    let (lo, hi) = bounds(piece_bounds.iter().flat_map(|&(a, b)| [a, b]));
    let gap = 0.02 * (hi - lo).max_element();
    if gap.is_nan() || gap <= 0. {
        return dropped;
    }

    // Which pieces lie within `gap` of each other, by a hash of vertex cells.
    let mut cells: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
    let cell = |p: Vec3| ((p - lo) / gap).floor().as_ivec3().to_array();
    for (k, piece) in pieces.iter().enumerate() {
        for &t in piece {
            for i in mesh.triangles[t] {
                let list = cells.entry(cell(mesh.positions[i as usize])).or_default();
                if list.last() != Some(&k) && !list.contains(&k) {
                    list.push(k);
                }
            }
        }
    }
    let mut near = vec![Vec::new(); pieces.len()];
    for (&[x, y, z], list) in &cells {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(other) = cells.get(&[x + dx, y + dy, z + dz]) else {
                        continue;
                    };
                    for &a in list {
                        for &b in other {
                            if a != b && !near[a].contains(&b) {
                                near[a].push(b);
                            }
                        }
                    }
                }
            }
        }
    }
    // `pieces` is sorted largest first; keep what is reachable from it.
    let mut kept = vec![false; pieces.len()];
    let mut stack = vec![0];
    kept[0] = true;
    while let Some(a) = stack.pop() {
        for &b in &near[a] {
            if !kept[b] {
                kept[b] = true;
                stack.push(b);
            }
        }
    }
    dropped.detached = (0..pieces.len())
        .filter(|&k| !kept[k])
        .map(|k| pieces[k].len())
        .sum();

    // Flat bases, against the bounds of the rest.
    for k in 1..pieces.len() {
        if !kept[k] {
            continue;
        }
        let (plo, phi) = piece_bounds[k];
        let (rlo, rhi) = bounds(
            (0..pieces.len())
                .filter(|&o| o != k && kept[o])
                .flat_map(|o| [piece_bounds[o].0, piece_bounds[o].1]),
        );
        let (psize, rsize) = (phi - plo, rhi - rlo);
        let base = (0..3).any(|a| {
            let flat = psize[a] <= 0.005 * rsize[a];
            let floor = (plo[a] - rlo[a]).abs() <= 0.01 * rsize[a]
                || (phi[a] - rhi[a]).abs() <= 0.01 * rsize[a]
                || phi[a] <= rlo[a]
                || plo[a] >= rhi[a];
            let wide = (0..3).any(|b| b != a && psize[b] >= 0.2 * rsize[b]);
            flat && floor && wide
        });
        if base {
            kept[k] = false;
            dropped.bases += pieces[k].len();
        }
    }

    let mut drop = vec![false; mesh.triangles.len()];
    for (k, piece) in pieces.iter().enumerate() {
        if !kept[k] {
            for &t in piece {
                drop[t] = true;
            }
        }
    }
    let mut i = 0;
    mesh.triangles.retain(|_| {
        let keep = !drop[i];
        i += 1;
        keep
    });
    if dropped.detached > 0 {
        mesh.notes.push(format!(
            "Left out {} triangles standing apart from the body (a prop or stray piece).",
            dropped.detached
        ));
    }
    if dropped.bases > 0 {
        mesh.notes.push(format!(
            "Left out {} triangles of a flat base under the feet (a ground plane or shadow disc).",
            dropped.bases
        ));
    }
    dropped
}
