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
