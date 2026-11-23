//! Roblox's own baked union mesh (`MeshData`/`MeshData2`, the CSGMDL
//! format, or the `SolidMeshHolder` current Studio saves instead, see
//! `rbx_csg::solidmesh`), read for a union whose operation tree is gone (no
//! `ChildData` inline and none in the `PartOperationAsset` its `AssetId`
//! names), or whose tree the boolean cannot carve. Anything with a tree is
//! carved from it first (`super::csg`).
//!
//! Nothing here comes from Roblox documentation, which does not describe the
//! format. It was worked out by reading real blobs: asset 305197512, and the
//! inline blobs of the places this project is tested against, whose unions
//! also carry their tree, so what they decode to could be checked against
//! the bake's own `InitialSize`. The one version 4 blob read is asset
//! 4500696697's, as the `rbx_mesh` crate's test meshes keep it.
//!
//! - **Verified:** every blob opens with `CSGMDL` and a `u32` version once
//!   unscrambled, and the scrambling is an XOR with the 31-byte repeating
//!   [`KEY`], indexed by the byte's position in the blob. Versions 2 and 4
//!   scramble the whole blob; version 5 only its 10-byte magic and version,
//!   leaving the rest plain. A wrong key cannot pass [`plain`] unnoticed.
//! - **Verified:** positions are in the union's own studs at its
//!   `InitialSize`, triangles are wound counter-clockwise like every other
//!   mesh here, and every decoded blob is a closed mesh. The layouts are
//!   described in [`v2`] (which reads version 4 too) and [`v5`].
//! - **Not used:** the texture coordinates, tangents and per-vertex face ids
//!   the blobs also carry. Texture coordinates are box-projected the way
//!   `super::csg` does instead, so a decoded union tiles its material exactly
//!   like a carved one.

mod v2;
mod v5;

use glam::Vec3;

const MAGIC: &[u8; 6] = b"CSGMDL";
const KEY: [u8; 31] = [
    0x56, 0x2e, 0x6e, 0x58, 0x31, 0x20, 0x30, 0x04, 0x34, 0x69, 0x0c, 0x77, 0x0c, 0x01, 0x5e, 0x00,
    0x1a, 0x60, 0x37, 0x69, 0x1d, 0x52, 0x2b, 0x07, 0x4f, 0x24, 0x59, 0x65, 0x53, 0x04, 0x7a,
];
/// The magic and the `u32` version.
const PREFIX: usize = 10;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    /// Neither the bytes nor their unscrambled form open with `CSGMDL`.
    NotCsg,
    /// A version this module does not read (see the module doc).
    Version(u32),
    /// The document ends before what its own counts promise.
    Truncated,
    /// A vertex too small to hold a position, normal and colour (version 2).
    Stride(u32),
    /// An index past the last vertex, a count that is not whole triangles,
    /// or an index code this module does not know (version 5).
    Index,
    /// Two of a document's per-vertex arrays disagree on the vertex count.
    Count,
}

/// The unscrambled document; `bytes` itself when it is already plain.
fn plain(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut document = bytes.to_vec();
    if bytes.starts_with(MAGIC) {
        return Ok(document);
    }
    let unscramble = |document: &mut Vec<u8>, range: std::ops::Range<usize>| {
        for at in range {
            document[at] ^= KEY[at % KEY.len()];
        }
    };
    if document.len() < PREFIX {
        return Err(Error::NotCsg);
    }
    unscramble(&mut document, 0..PREFIX);
    if !document.starts_with(MAGIC) {
        return Err(Error::NotCsg);
    }
    if matches!(document[MAGIC.len()..PREFIX], [2 | 4, 0, 0, 0]) {
        let len = document.len();
        unscramble(&mut document, PREFIX..len);
    }
    Ok(document)
}

/// A bounds-checked little-endian cursor over a plain document.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8], Error> {
        let end = self.at.checked_add(len).ok_or(Error::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(Error::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let raw = self.take(N)?;
        Ok(std::array::from_fn(|i| raw[i]))
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// `count` items of `size` bytes each, as one slice.
    fn items(&mut self, count: usize, size: usize) -> Result<&[u8], Error> {
        self.take(count.checked_mul(size).ok_or(Error::Truncated)?)
    }
}

fn f32s<const N: usize>(raw: &[u8]) -> [f32; N] {
    std::array::from_fn(|i| {
        f32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]])
    })
}

/// Reads `lods` `u32` offsets into `decoded`'s indices and keeps only the
/// span between the first two, the finest LOD.
fn first_lod(reader: &mut Reader, lods: usize, decoded: &mut Decoded) -> Result<(), Error> {
    let offsets: Vec<usize> = reader
        .items(lods, 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b) as usize)
        .collect();
    if let [first, second, ..] = offsets[..] {
        decoded.indices = decoded
            .indices
            .get(first..second)
            .ok_or(Error::Index)?
            .to_vec();
    }
    Ok(())
}

/// What a layout reads out of a document, before it becomes a mesh.
struct Decoded {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
}

/// A decoded union mesh, each corner in the colour it was baked in, what
/// paints it where `UsePartColor` is off.
pub(super) struct Baked {
    pub(super) mesh: rbx_mesh::Mesh,
}

/// A `SolidMeshHolder`'s triangles, each corner its own vertex: its
/// corners index positions and colours separately.
fn solid_mesh(mesh: rbx_csg::solidmesh::Decoded) -> Decoded {
    let mut decoded = Decoded {
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    for (triangle, [r, g, b]) in mesh.triangles.iter().zip(mesh.colors) {
        let corners = triangle.map(|i| Vec3::from(mesh.positions[i as usize]));
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or_zero()
            .to_array();
        for corner in corners {
            decoded.indices.push(decoded.positions.len() as u32);
            decoded.positions.push(corner.to_array());
            decoded.normals.push(normal);
            decoded.colors.push([r, g, b, 255]);
        }
    }
    decoded
}

/// Decodes one `MeshData` blob, scrambled or not, or a `SolidMeshHolder`.
pub(super) fn decode(bytes: &[u8]) -> Result<Baked, Error> {
    if let Some(mesh) = rbx_csg::solidmesh::decode(bytes) {
        return finish(solid_mesh(mesh));
    }
    let document = plain(bytes)?;
    let mut reader = Reader {
        bytes: &document,
        at: MAGIC.len(),
    };
    let decoded = match reader.u32()? {
        2 => v2::read(&mut reader)?,
        4 => v2::read_v4(&mut reader)?,
        5 => v5::read(&mut reader)?,
        version => return Err(Error::Version(version)),
    };
    finish(decoded)
}

fn finish(decoded: Decoded) -> Result<Baked, Error> {
    let Decoded {
        positions,
        normals,
        colors,
        indices,
    } = decoded;
    let count = positions.len();
    if normals.len() != count || colors.len() != count {
        return Err(Error::Count);
    }
    if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= count) {
        return Err(Error::Index);
    }

    let mut bounds = rbx_mesh::Aabb {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };
    let vertices: Vec<rbx_mesh::Vertex> = positions
        .into_iter()
        .zip(normals)
        .zip(colors)
        .map(|((position, normal), color)| {
            bounds.min = std::array::from_fn(|axis| bounds.min[axis].min(position[axis]));
            bounds.max = std::array::from_fn(|axis| bounds.max[axis].max(position[axis]));
            let (u, v) = rbx_csg::dominant_axes(normal);
            rbx_mesh::Vertex {
                position,
                normal,
                uv: [
                    position[u] / rbx_materials::DEFAULT_STUDS_PER_TILE,
                    position[v] / rbx_materials::DEFAULT_STUDS_PER_TILE,
                ],
                color,
            }
        })
        .collect();

    Ok(Baked {
        mesh: rbx_mesh::Mesh {
            // A CSGMDL version, not a `.mesh` header one: there is none to echo.
            version: (0, 0),
            vertices,
            indices,
            lods: Vec::new(),
            bounds,
        },
    })
}

/// The `SolidMeshHolder`, `MeshData2` or `MeshData` a union or
/// `PartOperationAsset` carries, the first one filled.
pub(super) fn mesh_data(
    properties: &std::collections::BTreeMap<String, rbx_dom::Variant>,
) -> Option<&[u8]> {
    ["SolidMeshHolder", "MeshData2", "MeshData"]
        .iter()
        .find_map(|key| match properties.get(*key) {
            // Never valid UTF-8, so always `Unknown`: see `tree::child_data`.
            Some(rbx_dom::Variant::Unknown { raw, .. }) if !raw.is_empty() => Some(raw.as_slice()),
            _ => None,
        })
}

/// The baked mesh of a union whose `bytes` are either a downloaded
/// `PartOperationAsset` document or the union's own inline `MeshData`.
pub(super) fn of_bytes(bytes: &[u8]) -> Option<Baked> {
    if let Ok(baked) = decode(bytes) {
        return Some(baked);
    }
    let dom = rbx_binary::deserialize(bytes).ok()?;
    let root = dom.get(*dom.root_refs().first()?)?;
    decode(mesh_data(root.properties())?).ok()
}

#[cfg(test)]
#[path = "baked/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "baked/v5_tests.rs"]
mod v5_tests;
