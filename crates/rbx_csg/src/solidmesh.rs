//! `SolidMeshHolder`, where current Studio keeps a union's carved mesh (see
//! `docs/research/csg-solidmeshholder.md`; the layout was read off blobs
//! Studio saved). Studio draws a union from it alone, so a union made here
//! must carry one for Studio to show it.
//!
//! A 14-byte header (`03 01`, the body length, a zero compressed size and
//! the body length again), then `SolidMesh` and a zero word. Four attribute
//! pools follow, each a `u64` count and its `f32`s: positions (xyz, in the
//! union's own space at `InitialSize`), normals (xyz), texture coordinates
//! (uv, empty for a union) and sRGB colours (rgb in 0..1). Then four index
//! streams in the same order, each a `u64` corner count and one
//! delta-coded index per triangle corner (see [`write_deltas`]): a corner
//! takes its position, normal, uv and colour from separate pools.

use std::collections::HashMap;

use glam::DVec3;

use crate::bsp::{Plane, Polygon};
use crate::Solid;

const TAG: &[u8; 9] = b"SolidMesh";
const HEADER: usize = 14;
const WIDE: u8 = 0x80;

/// One index stream: each value's delta from the last, 7-bit two's
/// complement in one byte, or `WIDE` and 23 bits big-endian in three. The
/// same coding CSGMDL's index stream uses.
pub(crate) fn write_deltas(out: &mut Vec<u8>, indices: impl IntoIterator<Item = u32>) {
    let mut current: i64 = 0;
    for index in indices {
        let delta = i64::from(index) - current;
        current = i64::from(index);
        if (-64..64).contains(&delta) {
            out.push((delta as u8) & 0x7f);
        } else {
            let word = (delta as u32) & 0x7f_ffff;
            out.extend([WIDE | (word >> 16) as u8, (word >> 8) as u8, word as u8]);
        }
    }
}

/// Reads `count` values written by [`write_deltas`] from the front of
/// `bytes`, each below `limit`; returns them and the bytes read.
pub(crate) fn read_deltas(bytes: &[u8], count: usize, limit: usize) -> Option<(Vec<u32>, usize)> {
    let mut out = Vec::with_capacity(count.min(bytes.len()));
    let mut at = 0;
    let mut current: i64 = 0;
    for _ in 0..count {
        let code = *bytes.get(at)?;
        let delta = if code & WIDE == 0 {
            at += 1;
            i64::from(((code << 1) as i8) >> 1)
        } else {
            let rest = bytes.get(at + 1..at + 3)?;
            at += 3;
            let word = u32::from_be_bytes([0, code & !WIDE, rest[0], rest[1]]);
            i64::from(((word << 9) as i32) >> 9)
        };
        current += delta;
        let index = u32::try_from(current).ok()?;
        if index as usize >= limit {
            return None;
        }
        out.push(index);
    }
    Some((out, at))
}

/// Hands out one index per distinct value, in first-seen order.
struct Pool<const N: usize> {
    values: Vec<[f32; N]>,
    seen: HashMap<[u32; N], u32>,
}

impl<const N: usize> Pool<N> {
    fn new() -> Self {
        Pool {
            values: Vec::new(),
            seen: HashMap::new(),
        }
    }

    fn index(&mut self, value: [f32; N]) -> u32 {
        let next = self.values.len() as u32;
        *self.seen.entry(value.map(f32::to_bits)).or_insert_with(|| {
            self.values.push(value);
            next
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend((self.values.len() as u64).to_le_bytes());
        for value in &self.values {
            for c in value {
                out.extend(c.to_le_bytes());
            }
        }
    }
}

fn write_stream(out: &mut Vec<u8>, corners: &[u32]) {
    out.extend((corners.len() as u64).to_le_bytes());
    write_deltas(out, corners.iter().copied());
}

/// How close two corners must be to become one vertex in the blob. Studio
/// only draws a closed mesh (anything else becomes a small stand-in box),
/// and the boolean can leave corners up to ~1e-3 apart across a sliver it
/// let through (see `repair::only_slivers`), wider than its own weld.
const WELD: f32 = 2e-3;

/// Hands out one position index per corner, reusing any vertex already
/// within [`WELD`] of it, its own grid cell's or a neighbour's.
#[derive(Default)]
struct Welder {
    values: Vec<[f32; 3]>,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

impl Welder {
    fn index(&mut self, value: [f32; 3]) -> u32 {
        let cell = value.map(|c| (c / WELD).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let near = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    for &i in self.cells.get(&near).into_iter().flatten() {
                        let other = self.values[i as usize];
                        if (0..3).all(|axis| (other[axis] - value[axis]).abs() <= WELD) {
                            return i;
                        }
                    }
                }
            }
        }
        let next = self.values.len() as u32;
        self.values.push(value);
        self.cells.entry(cell).or_default().push(next);
        next
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend((self.values.len() as u64).to_le_bytes());
        for value in &self.values {
            for c in value {
                out.extend(c.to_le_bytes());
            }
        }
    }
}

/// `solid` (in the union's own space) as a `SolidMeshHolder` blob, every
/// face flat-shaded in its polygon's colour, corners welded (see [`WELD`])
/// and the triangles that collapses left out.
pub fn encode(solid: &Solid) -> Vec<u8> {
    let (mut positions, mut normals, mut colors) = (Welder::default(), Pool::new(), Pool::new());
    let (mut position_corners, mut normal_corners, mut color_corners) =
        (Vec::new(), Vec::new(), Vec::new());
    for polygon in &solid.polygons {
        let normal = normals.index(polygon.plane.normal.as_vec3().to_array());
        let color = colors.index(polygon.color.map(|c| f32::from(c) / 255.0));
        let ring: Vec<u32> = polygon
            .vertices
            .iter()
            .map(|v| positions.index(v.as_vec3().to_array()))
            .collect();
        for corner in 1..ring.len().saturating_sub(1) {
            let triangle = [ring[0], ring[corner], ring[corner + 1]];
            if triangle[0] == triangle[1]
                || triangle[1] == triangle[2]
                || triangle[0] == triangle[2]
            {
                continue;
            }
            position_corners.extend(triangle);
            normal_corners.extend([normal; 3]);
            color_corners.extend([color; 3]);
        }
    }
    let mut body = TAG.to_vec();
    body.extend(0u32.to_le_bytes());
    positions.write(&mut body);
    normals.write(&mut body);
    Pool::<2>::new().write(&mut body);
    colors.write(&mut body);
    write_stream(&mut body, &position_corners);
    write_stream(&mut body, &normal_corners);
    write_stream(&mut body, &[]);
    write_stream(&mut body, &color_corners);

    let length = (body.len() as u32).to_le_bytes();
    let mut out = vec![3, 1];
    out.extend(length);
    out.extend(0u32.to_le_bytes());
    out.extend(length);
    out.extend(body);
    out
}

/// A decoded blob: positions, and per triangle its three position indices
/// and its colour (its first corner's).
pub struct Decoded {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub colors: Vec<[u8; 3]>,
}

impl Decoded {
    /// The mesh as a solid placed by `model`, each face its triangle's colour.
    pub fn solid(&self, model: glam::DMat4) -> Solid {
        let mirrored = model.determinant() < 0.0;
        let polygons = self
            .triangles
            .iter()
            .zip(&self.colors)
            .filter_map(|(triangle, &color)| {
                let [a, b, c] = triangle.map(|i| {
                    model.transform_point3(DVec3::from(self.positions[i as usize].map(f64::from)))
                });
                let (b, c) = if mirrored { (c, b) } else { (b, c) };
                Some(Polygon {
                    vertices: vec![a, b, c],
                    plane: Plane::from_points(a, b, c)?,
                    color,
                })
            })
            .collect();
        Solid { polygons }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn count(&mut self) -> Option<usize> {
        let raw = self.bytes.get(self.at..self.at + 8)?;
        self.at += 8;
        usize::try_from(u64::from_le_bytes(raw.try_into().ok()?)).ok()
    }

    fn floats<const N: usize>(&mut self) -> Option<Vec<[f32; N]>> {
        let count = self.count()?;
        let raw = self
            .bytes
            .get(self.at..self.at.checked_add(count.checked_mul(4 * N)?)?)?;
        self.at += raw.len();
        Some(
            (0..count)
                .map(|k| {
                    std::array::from_fn(|i| {
                        let at = (k * N + i) * 4;
                        f32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
                    })
                })
                .collect(),
        )
    }

    fn stream(&mut self, limit: usize) -> Option<Vec<u32>> {
        let count = self.count()?;
        let (values, read) = read_deltas(self.bytes.get(self.at..)?, count, limit)?;
        self.at += read;
        Some(values)
    }
}

/// Reads a blob Studio (or [`encode`]) wrote. `None` for anything else,
/// including a compressed body, which no blob seen so far has used.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let body = bytes.get(HEADER..)?;
    let declared = u32::from_le_bytes(bytes.get(2..6)?.try_into().ok()?) as usize;
    let compressed = u32::from_le_bytes(bytes.get(6..10)?.try_into().ok()?);
    if bytes[..2] != [3, 1] || compressed != 0 || declared != body.len() || !body.starts_with(TAG) {
        return None;
    }
    let mut r = Reader {
        bytes: body,
        at: TAG.len() + 4,
    };
    let positions = r.floats::<3>()?;
    let normals = r.floats::<3>()?;
    let uvs = r.floats::<2>()?;
    let colors = r.floats::<3>()?;
    let position_corners = r.stream(positions.len())?;
    let normal_corners = r.stream(normals.len())?;
    r.stream(uvs.len())?;
    let color_corners = r.stream(colors.len())?;
    if position_corners.len() % 3 != 0 || normal_corners.len() != position_corners.len() {
        return None;
    }
    let triangles: Vec<[u32; 3]> = position_corners.as_chunks::<3>().0.to_vec();
    let colors = (0..triangles.len())
        .map(|t| {
            color_corners.get(t * 3).map_or(crate::DEFAULT_COLOR, |&i| {
                colors[i as usize].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
            })
        })
        .collect();
    Some(Decoded {
        positions,
        triangles,
        colors,
    })
}

#[cfg(test)]
mod tests;
