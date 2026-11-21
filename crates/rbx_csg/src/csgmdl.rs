//! `MeshData2`, the baked union mesh Studio saves (the CSGMDL format),
//! version 5, the layout Studio writes now: what a union made here saves so
//! Studio and the viewer can draw it without carving it again. The layout
//! was worked out from real blobs; the viewer's `scene::union::baked`
//! documents it and reads every version.
//!
//! After the magic and the `u32` version (both XORed with [`KEY`] by byte
//! position), every per-vertex array opens with its `u16` vertex count:
//! positions, normals (`u32` byte length, `3 × u16` where 0 is −1 and 65534
//! is +1), RGBA colours, a source-face byte, texture coordinates and
//! tangents (in the normals' encoding). Then the index count, the byte
//! length of the delta-coded index stream, the stream, and the LOD table.

use glam::DVec3;

use crate::{dominant_axes, Solid};

const MAGIC: &[u8; 6] = b"CSGMDL";
const VERSION: u32 = 5;
const KEY: [u8; 31] = [
    0x56, 0x2e, 0x6e, 0x58, 0x31, 0x20, 0x30, 0x04, 0x34, 0x69, 0x0c, 0x77, 0x0c, 0x01, 0x5e, 0x00,
    0x1a, 0x60, 0x37, 0x69, 0x1d, 0x52, 0x2b, 0x07, 0x4f, 0x24, 0x59, 0x65, 0x53, 0x04, 0x7a,
];
const PREFIX: usize = 10;
const WIDE: u8 = 0x80;
const STUDS_PER_TILE: f32 = 4.0;

fn unit(value: f32) -> [u8; 2] {
    ((value.clamp(-1.0, 1.0) * 32767.0 + 32767.0).round() as u16).to_le_bytes()
}

fn units(out: &mut Vec<u8>, count: u16, vectors: &[[f32; 3]]) {
    out.extend(count.to_le_bytes());
    out.extend((vectors.len() as u32 * 6).to_le_bytes());
    for v in vectors {
        for c in v {
            out.extend(unit(*c));
        }
    }
}

/// `solid` (in the union's own space) as a version 5 blob, every vertex
/// coloured `color`. `None` past the format's 65535 vertices.
pub fn encode(solid: &Solid, color: [u8; 3]) -> Option<Vec<u8>> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for polygon in &solid.polygons {
        let base = positions.len() as u32;
        let normal = polygon.plane.normal.as_vec3().to_array();
        for v in &polygon.vertices {
            positions.push(v.as_vec3().to_array());
            normals.push(normal);
        }
        for corner in 1..polygon.vertices.len().saturating_sub(1) as u32 {
            indices.extend([base, base + corner, base + corner + 1]);
        }
    }
    let count = u16::try_from(positions.len()).ok()?;
    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    for (at, byte) in out.iter_mut().enumerate() {
        *byte ^= KEY[at % KEY.len()];
    }
    out.extend(count.to_le_bytes());
    for p in &positions {
        for c in p {
            out.extend(c.to_le_bytes());
        }
    }
    units(&mut out, count, &normals);
    out.extend(count.to_le_bytes());
    for _ in 0..count {
        out.extend([color[0], color[1], color[2], 255]);
    }
    out.extend(count.to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, count.into()));
    out.extend(count.to_le_bytes());
    for (p, n) in positions.iter().zip(&normals) {
        let (u, v) = dominant_axes(*n);
        out.extend((p[u] / STUDS_PER_TILE).to_le_bytes());
        out.extend((p[v] / STUDS_PER_TILE).to_le_bytes());
    }
    let tangents: Vec<[f32; 3]> = normals
        .iter()
        .map(|n| {
            DVec3::from(n.map(f64::from))
                .any_orthonormal_vector()
                .as_vec3()
                .to_array()
        })
        .collect();
    units(&mut out, count, &tangents);

    let mut stream = Vec::new();
    let mut current: i64 = 0;
    for &index in &indices {
        let delta = i64::from(index) - current;
        current = i64::from(index);
        if (-64..64).contains(&delta) {
            stream.push((delta as u8) & 0x7f);
        } else {
            let word = (delta as u32) & 0x7f_ffff;
            stream.extend([WIDE | (word >> 16) as u8, (word >> 8) as u8, word as u8]);
        }
    }
    out.extend((indices.len() as u32).to_le_bytes());
    out.extend((stream.len() as u32).to_le_bytes());
    out.extend(stream);
    out.push(2);
    out.extend(0u32.to_le_bytes());
    out.extend((indices.len() as u32).to_le_bytes());
    Some(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Option<&[u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(len)?)?;
        self.at += len;
        Some(slice)
    }

    fn u16(&mut self) -> Option<usize> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?).into())
    }

    fn u32(&mut self) -> Option<usize> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize)
    }

    fn count(&mut self, expected: usize) -> Option<()> {
        (self.u16()? == expected).then_some(())
    }
}

/// The triangles of a version 5 blob (positions and indices, first LOD),
/// enough to carve or collide with a union saved by us or by Studio.
pub fn decode(bytes: &[u8]) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let mut prefix: Vec<u8> = bytes.get(..PREFIX)?.to_vec();
    if !prefix.starts_with(MAGIC) {
        for (at, byte) in prefix.iter_mut().enumerate() {
            *byte ^= KEY[at % KEY.len()];
        }
    }
    if !prefix.starts_with(MAGIC) || prefix[6..] != VERSION.to_le_bytes() {
        return None;
    }
    let mut r = Reader { bytes, at: PREFIX };
    let count = r.u16()?;
    let positions = r
        .take(count * 12)?
        .as_chunks::<12>()
        .0
        .iter()
        .map(|raw| {
            std::array::from_fn(|i| f32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect();
    r.count(count)?;
    let length = r.u32()?;
    r.take(length)?;
    r.count(count)?;
    r.take(count * 4)?;
    r.count(count)?;
    r.take(count)?;
    r.count(count)?;
    r.take(count * 8)?;
    r.count(count)?;
    let length = r.u32()?;
    r.take(length)?;
    let index_count = r.u32()?;
    let length = r.u32()?;
    let mut stream = Reader {
        bytes: r.take(length)?,
        at: 0,
    };
    let mut indices = Vec::with_capacity(index_count.min(length));
    let mut current: i64 = 0;
    for _ in 0..index_count {
        let code = stream.take(1)?[0];
        let delta = match code & WIDE {
            0 => i64::from(((code << 1) as i8) >> 1),
            _ => {
                let rest = stream.take(2)?;
                let word = u32::from_be_bytes([0, code & !WIDE, rest[0], rest[1]]);
                i64::from(((word << 9) as i32) >> 9)
            }
        };
        current += delta;
        let index = u32::try_from(current).ok()?;
        if index as usize >= count {
            return None;
        }
        indices.push(index);
    }
    let lods = r.take(1)?[0] as usize;
    let offsets: Vec<usize> = (0..lods).map(|_| r.u32()).collect::<Option<_>>()?;
    if let [first, second, ..] = offsets[..] {
        indices = indices.get(first..second)?.to_vec();
    }
    Some((positions, indices))
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec3};

    use super::*;
    use crate::shapes;

    #[test]
    fn a_carved_solid_survives_the_round_trip() {
        let a = Solid::from_mesh(&shapes::block(), Mat4::from_scale(Vec3::splat(100.0)));
        let b = Solid::from_mesh(
            &shapes::sphere(),
            Mat4::from_translation(Vec3::splat(50.0)) * Mat4::from_scale(Vec3::splat(60.0)),
        );
        let solid = a.subtract(b).unwrap().finish(None).unwrap();
        let blob = encode(&solid, [10, 20, 30]).unwrap();
        assert_ne!(&blob[..6], MAGIC, "the prefix is scrambled as Studio's is");
        let (positions, indices) = decode(&blob).unwrap();
        let back = Solid::from_triangles(&positions, &indices, glam::DMat4::IDENTITY);
        assert!((back.volume() - solid.volume()).abs() < 1e-2);
    }

    #[test]
    fn every_piece_of_a_many_piece_solid_comes_back() {
        let mut solid = Solid::default();
        for i in 0..40 {
            solid.polygons.extend(
                Solid::from_mesh(
                    &shapes::block(),
                    Mat4::from_translation(Vec3::new(i as f32 * 3.0, 0.0, 0.0)),
                )
                .polygons,
            );
        }
        let blob = encode(&solid, [0; 3]).unwrap();
        let (positions, indices) = decode(&blob).unwrap();
        assert_eq!(positions.len(), 40 * 24);
        assert_eq!(indices.len(), 40 * 36);
    }
}
