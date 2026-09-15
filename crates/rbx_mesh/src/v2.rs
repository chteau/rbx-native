//! `version 2.00`/`2.01` and `3.00`/`3.01`: the first binary layout, little-endian.
//!
//! v3 extends the v2 header with a LOD table descriptor and appends the table
//! itself after the face list; the vertex and face records are identical.

use crate::error::MeshError;
use crate::mesh::Mesh;
use crate::reader::Reader;
use crate::records::{self, FACE_SIZE};

const V2_HEADER_SIZE: u16 = 12;
const V3_HEADER_SIZE: u16 = 16;

// A v3 LOD entry is a single u32 face offset. The spec calls this header field
// "unused, always 4"; it is still honoured, so a file that disagrees is skipped
// over rather than misread.
const LOD_ENTRY_SIZE: u16 = 4;

struct Layout {
    vertex_size: u8,
    num_verts: usize,
    num_faces: usize,
    num_lods: usize,
    lod_entry_size: u16,
}

pub(crate) fn parse(version: (u8, u8), body: &[u8]) -> Result<Mesh, MeshError> {
    let mut reader = Reader::new(body);
    let layout = read_header(&mut reader, version.0)?;

    let vertices = records::read_vertices(&mut reader, layout.num_verts, layout.vertex_size)?;
    let faces = records::read_faces(&mut reader, layout.num_faces)?;
    let lod_table = read_lod_table(&mut reader, &layout)?;
    reader.expect_eof()?;

    Mesh::assemble(version, vertices, &faces, &lod_table)
}

fn read_header(reader: &mut Reader<'_>, major: u8) -> Result<Layout, MeshError> {
    let expected = if major == 2 {
        V2_HEADER_SIZE
    } else {
        V3_HEADER_SIZE
    };

    let header_size = reader.u16()?;
    if header_size != expected {
        return Err(MeshError::UnexpectedHeaderSize {
            expected,
            actual: header_size,
        });
    }

    let vertex_size = reader.u8()?;
    let face_size = reader.u8()?;
    if face_size != FACE_SIZE {
        return Err(MeshError::UnsupportedFaceSize(face_size));
    }
    records::check_vertex_size(vertex_size)?;

    // The two LOD fields sit between the strides and the counts in v3, not after
    // them: the counts are the last thing in both headers. Both fields read 4 in
    // the sample file, so the order is taken from the spec and from the reading
    // order of the rbx_mesh/Source2Roblox implementations, not from these bytes.
    let (lod_entry_size, num_lods) = if major == 2 {
        (0, 0)
    } else {
        (reader.u16()?, reader.u16()? as usize)
    };

    Ok(Layout {
        vertex_size,
        num_verts: reader.u32()? as usize,
        num_faces: reader.u32()? as usize,
        num_lods,
        lod_entry_size,
    })
}

fn read_lod_table(reader: &mut Reader<'_>, layout: &Layout) -> Result<Vec<u32>, MeshError> {
    if layout.num_lods == 0 {
        return Ok(Vec::new());
    }
    if layout.lod_entry_size != LOD_ENTRY_SIZE {
        // Stride is unknown to us; consume the block so nothing downstream
        // misaligns, and report no LOD table.
        reader.take_records(layout.num_lods, layout.lod_entry_size as usize)?;
        return Ok(Vec::new());
    }
    reader.u32_array(layout.num_lods)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex_bytes(x: f32, with_color: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [x, 2.0, 3.0, 0.0, 1.0, 0.0, 0.25, 0.75] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&[0x7F, 0x7F, 0x7F, 0x00]); // tangent
        if with_color {
            out.extend_from_slice(&[0x10, 0x20, 0x30, 0x40]);
        }
        out
    }

    fn v2_body(with_color: bool) -> Vec<u8> {
        let stride = if with_color { 40u8 } else { 36 };
        let mut body = Vec::new();
        body.extend_from_slice(&V2_HEADER_SIZE.to_le_bytes());
        body.push(stride);
        body.push(FACE_SIZE);
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&1u32.to_le_bytes());
        for corner in 0..3 {
            body.extend_from_slice(&vertex_bytes(corner as f32, with_color));
        }
        for index in 0u32..3 {
            body.extend_from_slice(&index.to_le_bytes());
        }
        body
    }

    // Two LODs over four faces: [0,2) then [2,4).
    fn v3_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&V3_HEADER_SIZE.to_le_bytes());
        body.push(40);
        body.push(FACE_SIZE);
        body.extend_from_slice(&LOD_ENTRY_SIZE.to_le_bytes());
        body.extend_from_slice(&3u16.to_le_bytes()); // three boundaries, two LODs
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&4u32.to_le_bytes());
        for corner in 0..3 {
            body.extend_from_slice(&vertex_bytes(corner as f32, true));
        }
        for _ in 0..4 {
            for index in 0u32..3 {
                body.extend_from_slice(&index.to_le_bytes());
            }
        }
        for offset in [0u32, 2, 4] {
            body.extend_from_slice(&offset.to_le_bytes());
        }
        body
    }

    #[test]
    fn v2_with_a_36_byte_stride_defaults_the_color_to_white() {
        let mesh = parse((2, 0), &v2_body(false)).unwrap();

        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.vertices[0].color, [255, 255, 255, 255]);
        assert_eq!(mesh.vertices[1].position, [1.0, 2.0, 3.0]);
        assert_eq!(mesh.vertices[0].normal, [0.0, 1.0, 0.0]);
        assert_eq!(mesh.vertices[0].uv, [0.25, 0.75]);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    #[test]
    fn v2_with_a_40_byte_stride_reads_the_color_channel() {
        let mesh = parse((2, 0), &v2_body(true)).unwrap();
        assert_eq!(mesh.vertices[0].color, [0x10, 0x20, 0x30, 0x40]);
    }

    #[test]
    fn v3_splits_the_faces_into_lod_ranges_and_keeps_lod0() {
        let mesh = parse((3, 0), &v3_body()).unwrap();

        assert_eq!(mesh.lods, vec![0..2, 2..4]);
        // Two faces of three corners, not all four faces.
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn a_wrong_header_size_is_rejected_per_version() {
        let mut body = v2_body(true);
        body[0] = 16;
        assert!(matches!(
            parse((2, 0), &body),
            Err(MeshError::UnexpectedHeaderSize {
                expected: 12,
                actual: 16
            })
        ));
        // A v2 header offered as v3 fails the same check.
        assert!(matches!(
            parse((3, 0), &v2_body(true)),
            Err(MeshError::UnexpectedHeaderSize {
                expected: 16,
                actual: 12
            })
        ));
    }

    #[test]
    fn an_unknown_vertex_or_face_stride_is_rejected() {
        let mut body = v2_body(true);
        body[2] = 44;
        assert!(matches!(
            parse((2, 0), &body),
            Err(MeshError::UnsupportedVertexSize(44))
        ));

        let mut body = v2_body(true);
        body[3] = 6;
        assert!(matches!(
            parse((2, 0), &body),
            Err(MeshError::UnsupportedFaceSize(6))
        ));
    }

    #[test]
    fn an_inflated_vertex_count_errors_instead_of_allocating() {
        let mut body = v2_body(true);
        body[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse((2, 0), &body),
            Err(MeshError::TooShort { .. })
        ));
    }

    #[test]
    fn an_unreadable_lod_stride_drops_the_table_without_misaligning() {
        let mut body = v3_body();
        // Claim 6-byte entries; the table bytes are consumed, no ranges reported.
        body[4..6].copy_from_slice(&6u16.to_le_bytes());
        body[6..8].copy_from_slice(&2u16.to_le_bytes());
        let mesh = parse((3, 0), &body).unwrap();

        assert!(mesh.lods.is_empty());
        assert_eq!(mesh.indices.len(), 12);
    }

    #[test]
    fn bytes_past_the_last_declared_block_are_rejected() {
        let mut body = v2_body(true);
        body.push(0);
        assert!(matches!(
            parse((2, 0), &body),
            Err(MeshError::TrailingBytes(1))
        ));
    }

    #[test]
    fn every_truncation_of_a_valid_body_errors_without_panicking() {
        for body in [v2_body(false), v2_body(true), v3_body()] {
            for len in 0..body.len() {
                let _ = parse((2, 0), &body[..len]);
                let _ = parse((3, 0), &body[..len]);
            }
        }
    }
}
