//! `version 6.00` and `7.00`: a flat stream of tagged chunks read to EOF.
//!
//! Each chunk is an 8-byte NUL-padded type, a `u32` chunk version, a `u32`
//! payload size and the payload. Only `COREMESH` and `LODS` are read; `SKINNING`,
//! `FACS`, `HSRAVIS` and any unknown type are skipped by size. The two file
//! versions differ in the `COREMESH` chunk version alone: 1 holds the v4 vertex
//! and face records verbatim, 2 a Draco bitstream. Dispatching on the chunk
//! version rather than the file version keeps one reader for both.

use crate::draco;
use crate::error::MeshError;
use crate::mesh::{Mesh, Vertex};
use crate::reader::Reader;
use crate::records::{self, VERTEX_SIZE_COLORED};

pub(crate) fn parse(version: (u8, u8), body: &[u8]) -> Result<Mesh, MeshError> {
    let mut reader = Reader::new(body);
    let mut core = None;
    let mut lod_table = Vec::new();

    while reader.remaining() > 0 {
        let tag = reader.take(8)?;
        let chunk_version = reader.u32()?;
        let size = reader.u32()? as usize;
        let mut payload = Reader::new(reader.take(size)?);

        match (tag, chunk_version) {
            (b"COREMESH", 1) => core = Some(read_records(&mut payload)?),
            (b"COREMESH", 2) => {
                let len = payload.u32()? as usize;
                core = Some(draco::decode(payload.take(len)?)?);
            }
            (b"LODS\0\0\0\0", 1) => lod_table = read_lods(&mut payload)?,
            (b"COREMESH" | b"LODS\0\0\0\0", _) => {
                return Err(MeshError::UnsupportedChunkVersion {
                    chunk: String::from_utf8_lossy(tag)
                        .trim_end_matches('\0')
                        .to_owned(),
                    version: chunk_version,
                });
            }
            _ => continue,
        }
        // Same reasoning as `Reader::expect_eof`: a chunk that does not balance
        // means a field width is wrong, even if the values look plausible.
        payload.expect_eof()?;
    }

    let (vertices, faces) = core.ok_or(MeshError::MissingCoreMesh)?;
    Mesh::assemble(version, vertices, &faces, &lod_table)
}

fn read_records(payload: &mut Reader<'_>) -> Result<(Vec<Vertex>, Vec<[u32; 3]>), MeshError> {
    let num_verts = payload.u32()? as usize;
    let vertices = records::read_vertices(payload, num_verts, VERTEX_SIZE_COLORED)?;
    let num_faces = payload.u32()? as usize;
    Ok((vertices, records::read_faces(payload, num_faces)?))
}

fn read_lods(payload: &mut Reader<'_>) -> Result<Vec<u32>, MeshError> {
    // Same fields as the v4 header's LOD description, regrouped. Both are
    // ignored for the reason given in `v4::read_header`.
    let _lod_type = payload.u16()?;
    let _num_high_quality_lods = payload.u8()?;
    let count = payload.u32()? as usize;
    payload.u32_array(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draco::tests::{encode, quad};

    fn chunk(tag: &[u8; 8], version: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = tag.to_vec();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn lods(table: &[u32]) -> Vec<u8> {
        let mut payload = vec![0, 0, 1];
        payload.extend_from_slice(&(table.len() as u32).to_le_bytes());
        payload.extend(table.iter().flat_map(|offset| offset.to_le_bytes()));
        chunk(b"LODS\0\0\0\0", 1, &payload)
    }

    fn records_payload(vertices: &[Vertex], faces: &[[u32; 3]]) -> Vec<u8> {
        let mut payload = (vertices.len() as u32).to_le_bytes().to_vec();
        for vertex in vertices {
            let floats = vertex
                .position
                .iter()
                .chain(&vertex.normal)
                .chain(&vertex.uv);
            payload.extend(floats.flat_map(|value| value.to_le_bytes()));
            payload.extend_from_slice(&[0x7F, 0, 0, 0x7F]);
            payload.extend_from_slice(&vertex.color);
        }
        payload.extend_from_slice(&(faces.len() as u32).to_le_bytes());
        payload.extend(faces.iter().flatten().flat_map(|index| index.to_le_bytes()));
        payload
    }

    fn v6_body() -> Vec<u8> {
        let (vertices, faces) = quad();
        let mut body = chunk(b"COREMESH", 1, &records_payload(&vertices, &faces));
        body.extend(lods(&[0, 1, 2]));
        body.extend(chunk(b"SKINNING", 1, &[0; 12]));
        body
    }

    fn v7_body() -> Vec<u8> {
        let (vertices, faces) = quad();
        let blob = encode(&vertices, &faces, true);
        let mut payload = (blob.len() as u32).to_le_bytes().to_vec();
        payload.extend(blob);
        let mut body = chunk(b"COREMESH", 2, &payload);
        body.extend(lods(&[0, 1, 2]));
        body.extend(chunk(b"FACS\0\0\0\0", 1, &[0; 4]));
        body
    }

    #[test]
    fn v6_reads_its_uncompressed_records_and_the_lod_table() {
        let mesh = parse((6, 0), &v6_body()).unwrap();
        assert_eq!(mesh.vertices, quad().0);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.lods, vec![0..1, 1..2]);
    }

    #[test]
    fn a_version_6_file_decodes_through_the_public_header_entry() {
        let mut file = b"version 6.00\n".to_vec();
        file.extend(v6_body());
        let mesh = crate::parse(&file).unwrap();
        assert_eq!(mesh.vertices, quad().0);
        assert_eq!(mesh.lods, vec![0..1, 1..2]);
        assert_eq!(mesh.bounds.size(), [2.0, 0.5, 0.0]);
    }

    #[test]
    fn v7_decodes_its_draco_geometry_and_keeps_the_face_order() {
        let mesh = parse((7, 0), &v7_body()).unwrap();
        assert_eq!(mesh.vertices, quad().0);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.lods, vec![0..1, 1..2]);
        assert_eq!(mesh.bounds.size(), [2.0, 0.5, 0.0]);
    }

    #[test]
    fn a_degenerate_lod_table_keeps_every_face() {
        let (vertices, faces) = quad();
        let mut body = chunk(b"COREMESH", 1, &records_payload(&vertices, &faces));
        // What real v7 files carry: two zero offsets.
        body.extend(lods(&[0, 0]));
        assert_eq!(parse((6, 0), &body).unwrap().triangle_count(), 2);
    }

    #[test]
    fn a_file_without_geometry_is_rejected() {
        assert!(matches!(
            parse((7, 0), &lods(&[0, 0])),
            Err(MeshError::MissingCoreMesh)
        ));
    }

    #[test]
    fn an_unknown_coremesh_version_is_rejected() {
        assert!(matches!(
            parse((7, 0), &chunk(b"COREMESH", 3, &[])),
            Err(MeshError::UnsupportedChunkVersion { version: 3, .. })
        ));
    }

    #[test]
    fn a_chunk_with_bytes_past_its_fields_is_rejected() {
        let mut payload = records_payload(&quad().0, &quad().1);
        payload.push(0);
        assert!(matches!(
            parse((6, 0), &chunk(b"COREMESH", 1, &payload)),
            Err(MeshError::TrailingBytes(1))
        ));
    }

    #[test]
    fn every_truncation_and_mutation_errors_without_panicking() {
        for body in [v6_body(), v7_body()] {
            for len in 0..body.len() {
                let _ = parse((7, 0), &body[..len]);
            }
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            for _ in 0..400 {
                let mut damaged = body.clone();
                for _ in 0..4 {
                    state ^= state >> 12;
                    state ^= state << 25;
                    state ^= state >> 27;
                    let index =
                        (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize % damaged.len();
                    damaged[index] = (state >> 8) as u8;
                }
                let _ = parse((7, 0), &damaged);
            }
        }
    }
}
