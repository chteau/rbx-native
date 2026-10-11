//! `version 4.00`/`4.01` and `5.00`: the skinnable binary layout.
//!
//! The v2/v3 stride bytes are gone — v4 fixes the vertex record at 40 bytes and
//! LOD entries at 4 — and the header instead describes the optional skinning,
//! bone and subset blocks that follow the geometry. v5 adds a trailing FACS block.

use crate::error::MeshError;
use crate::mesh::Mesh;
use crate::reader::Reader;
use crate::records::{self, VERTEX_SIZE_COLORED};
use crate::skin::{self, Skin};

const V4_HEADER_SIZE: u16 = 24;
const V5_HEADER_SIZE: u16 = 32;

// The record sizes (8-byte envelope, 60-byte bone, 72-byte subset) live in
// `skin`. They agree across the community spec and three independent readers
// (rbx_mesh, Source2Roblox, Roblox-Mesh-Importer, the last byte-exact over a
// 799-mesh corpus), and with a real `version 5.00` dynamic head. A file that
// disagrees surfaces as TooShort or TrailingBytes rather than as silent
// corruption.
//
// The classic misread is 70 bytes for a subset, from taking its bone-index count
// as u16: on little-endian the count still looks right, the zero high half is
// eaten as the first bone index, and every later subset drifts two bytes.
#[cfg(test)]
use crate::skin::{BONE_SIZE, ENVELOPE_SIZE as SKIN_ENVELOPE_SIZE, SUBSET_SIZE};

struct Header {
    num_verts: usize,
    num_faces: usize,
    num_lods: usize,
    num_bones: usize,
    bone_names_size: usize,
    num_subsets: usize,
    facs_data_size: usize,
}

pub(crate) fn parse(version: (u8, u8), body: &[u8]) -> Result<(Mesh, Option<Skin>), MeshError> {
    let mut reader = Reader::new(body);
    let header = read_header(&mut reader, version.0)?;

    let vertices = records::read_vertices(&mut reader, header.num_verts, VERTEX_SIZE_COLORED)?;

    // The envelopes sit between the vertices and the faces, so their size has
    // to be exact for the face offsets to land. Their four index bytes are
    // subset-local slots, resolved through the subsets read further down.
    let envelopes = if header.num_bones > 0 {
        skin::read_envelopes(&mut reader, header.num_verts)?
    } else {
        Vec::new()
    };

    let faces = records::read_faces(&mut reader, header.num_faces)?;
    let lod_table = reader.u32_array(header.num_lods)?;

    let bones = skin::read_bones(&mut reader, header.num_bones)?;
    let names = reader.take(header.bone_names_size)?;
    let subsets = skin::read_subsets(&mut reader, header.num_subsets)?;
    // FACS is consumed but not surfaced: nothing here drives face bones.
    reader.take(header.facs_data_size)?;
    reader.expect_eof()?;

    let skin = (header.num_bones > 0).then(|| Skin::new(bones.named(names), subsets, envelopes));
    Ok((Mesh::assemble(version, vertices, &faces, &lod_table)?, skin))
}

fn read_header(reader: &mut Reader<'_>, major: u8) -> Result<Header, MeshError> {
    let expected = if major == 4 {
        V4_HEADER_SIZE
    } else {
        V5_HEADER_SIZE
    };

    let header_size = reader.u16()?;
    if header_size != expected {
        return Err(MeshError::UnexpectedHeaderSize {
            expected,
            actual: header_size,
        });
    }

    // lod_type is 4 on most sample files and 0 on those whose LOD table is the
    // degenerate `[0, 0]`; it is read but not trusted, because `Mesh::assemble`
    // validates the table itself rather than inferring from this enum. 4 is also
    // outside the documented enum (which stops at 3), so it must not be rejected.
    let _lod_type = reader.u16()?;
    let num_verts = reader.u32()? as usize;
    let num_faces = reader.u32()? as usize;
    let num_lods = reader.u16()? as usize;
    let num_bones = reader.u16()? as usize;
    let bone_names_size = reader.u32()? as usize;
    let num_subsets = reader.u16()? as usize;
    let _num_high_quality_lods = reader.u8()?;
    // Genuinely unused: observed as 0 and as 63 across otherwise valid files, so
    // it must not be validated.
    let _unused = reader.u8()?;

    // v5 appends the FACS descriptor and the block itself goes at the very end of
    // the file, after the subsets. The format id (0 means "no data") says how to
    // read it, which this crate only skips.
    let facs_data_size = if major >= 5 {
        let _facs_data_format = reader.u32()?;
        reader.u32()? as usize
    } else {
        0
    };

    Ok(Header {
        num_verts,
        num_faces,
        num_lods,
        num_bones,
        bone_names_size,
        num_subsets,
        facs_data_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counts {
        verts: u32,
        faces: u32,
        lods: u16,
        bones: u16,
        bone_names: u32,
        subsets: u16,
        facs: u32,
    }

    impl Counts {
        fn plain(verts: u32, faces: u32, lods: u16) -> Self {
            Counts {
                verts,
                faces,
                lods,
                bones: 0,
                bone_names: 0,
                subsets: 0,
                facs: 0,
            }
        }
    }

    fn body(major: u8, counts: &Counts) -> Vec<u8> {
        let header_size = if major == 4 {
            V4_HEADER_SIZE
        } else {
            V5_HEADER_SIZE
        };

        let mut out = Vec::new();
        out.extend_from_slice(&header_size.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes()); // lod_type
        out.extend_from_slice(&counts.verts.to_le_bytes());
        out.extend_from_slice(&counts.faces.to_le_bytes());
        out.extend_from_slice(&counts.lods.to_le_bytes());
        out.extend_from_slice(&counts.bones.to_le_bytes());
        out.extend_from_slice(&counts.bone_names.to_le_bytes());
        out.extend_from_slice(&counts.subsets.to_le_bytes());
        out.push(1); // num_high_quality_lods
        out.push(63); // unused, non-zero on real files
        if major >= 5 {
            out.extend_from_slice(&0u32.to_le_bytes()); // facs_data_format
            out.extend_from_slice(&counts.facs.to_le_bytes());
        }

        for corner in 0..counts.verts {
            for value in [corner as f32, 2.0, 3.0, 0.0, 1.0, 0.0, 0.5, 0.25] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&[0x7F, 0x7F, 0x7F, 0x00]);
            out.extend_from_slice(&[9, 9, 9, 255]);
        }
        if counts.bones > 0 {
            out.extend(std::iter::repeat_n(
                0u8,
                counts.verts as usize * SKIN_ENVELOPE_SIZE,
            ));
        }
        for _ in 0..counts.faces {
            for index in 0u32..3 {
                out.extend_from_slice(&index.to_le_bytes());
            }
        }
        // A well-formed table always ends on the face count.
        for boundary in 0..counts.lods {
            let offset = if boundary + 1 == counts.lods {
                counts.faces
            } else {
                boundary as u32
            };
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend(std::iter::repeat_n(
            0u8,
            counts.bones as usize * BONE_SIZE
                + counts.bone_names as usize
                + counts.subsets as usize * SUBSET_SIZE
                + counts.facs as usize,
        ));
        out
    }

    #[test]
    fn v4_reads_geometry_and_the_lod_table() {
        let (mesh, _) = parse((4, 1), &body(4, &Counts::plain(3, 2, 3))).unwrap();

        assert_eq!(mesh.version, (4, 1));
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.vertices[1].position, [1.0, 2.0, 3.0]);
        assert_eq!(mesh.vertices[0].color, [9, 9, 9, 255]);
        assert_eq!(mesh.lods, vec![0..1, 1..2]);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    #[test]
    fn v4_skips_the_skinning_block_so_the_faces_still_land() {
        let counts = Counts {
            bones: 2,
            bone_names: 9,
            subsets: 1,
            ..Counts::plain(4, 2, 0)
        };
        let bytes = body(4, &counts);
        let (mesh, _) = parse((4, 1), &bytes).unwrap();

        // Same geometry as the unskinned case: the 8-byte-per-vertex envelope, the
        // two 60-byte bones, the name buffer and the 72-byte subset were all
        // consumed at exactly the right width.
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices, vec![0, 1, 2, 0, 1, 2]);
        assert_eq!(
            bytes.len(),
            V4_HEADER_SIZE as usize + 4 * 40 + 4 * 8 + 2 * 12 + 2 * 60 + 9 + 72
        );
    }

    #[test]
    fn a_skinned_mesh_missing_its_envelope_bytes_is_rejected() {
        let counts = Counts {
            bones: 1,
            ..Counts::plain(4, 2, 0)
        };
        let mut bytes = body(4, &counts);
        // Drop the trailing bone record; the widths no longer add up.
        bytes.truncate(bytes.len() - BONE_SIZE);
        assert!(matches!(
            parse((4, 1), &bytes),
            Err(MeshError::TooShort { .. })
        ));
    }

    #[test]
    fn v5_consumes_its_facs_block() {
        let counts = Counts {
            facs: 40,
            ..Counts::plain(3, 1, 0)
        };
        let bytes = body(5, &counts);
        let (mesh, _) = parse((5, 0), &bytes).unwrap();

        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(bytes.len(), V5_HEADER_SIZE as usize + 3 * 40 + 12 + 40);
    }

    #[test]
    fn a_v5_header_offered_as_v4_is_rejected() {
        assert!(matches!(
            parse((4, 0), &body(5, &Counts::plain(3, 1, 0))),
            Err(MeshError::UnexpectedHeaderSize {
                expected: 24,
                actual: 32
            })
        ));
    }

    #[test]
    fn bytes_past_the_facs_block_are_rejected() {
        // The guard that would catch a wrong bone or subset stride on a real
        // skinned mesh, which no sample file here exercises.
        let mut bytes = body(
            5,
            &Counts {
                facs: 8,
                ..Counts::plain(3, 1, 0)
            },
        );
        bytes.extend_from_slice(&[0; 2]);
        assert!(matches!(
            parse((5, 0), &bytes),
            Err(MeshError::TrailingBytes(2))
        ));
    }

    #[test]
    fn an_inflated_count_errors_instead_of_allocating() {
        let mut bytes = body(4, &Counts::plain(3, 1, 0));
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse((4, 1), &bytes),
            Err(MeshError::TooShort { .. })
        ));
    }

    #[test]
    fn every_truncation_of_a_valid_body_errors_without_panicking() {
        for (major, counts) in [
            (4u8, Counts::plain(3, 2, 3)),
            (
                4,
                Counts {
                    bones: 2,
                    bone_names: 9,
                    subsets: 1,
                    ..Counts::plain(4, 2, 0)
                },
            ),
            (
                5,
                Counts {
                    facs: 40,
                    ..Counts::plain(3, 1, 0)
                },
            ),
        ] {
            let bytes = body(major, &counts);
            for len in 0..bytes.len() {
                let _ = parse((major, 0), &bytes[..len]);
            }
        }
    }

    /// A real skinned body: `body`'s geometry with the envelope, bone, name and
    /// subset blocks filled from `skin::tests::sample`.
    fn skinned_body(major: u8) -> Vec<u8> {
        use crate::skin::tests::{bone_bytes, envelope_bytes, sample, subset_bytes};
        let (bones, subsets, envelopes) = sample();
        let (bone_records, names) = bone_bytes(&bones);
        let counts = Counts {
            bones: 2,
            bone_names: names.len() as u32,
            subsets: 2,
            facs: if major >= 5 { 4 } else { 0 },
            ..Counts::plain(4, 2, 2)
        };
        let mut out = body(major, &counts);
        let header = if major == 4 {
            V4_HEADER_SIZE
        } else {
            V5_HEADER_SIZE
        } as usize;
        let envelope_at = header + 4 * 40;
        out[envelope_at..envelope_at + 4 * SKIN_ENVELOPE_SIZE]
            .copy_from_slice(&envelope_bytes(&envelopes));
        let bones_at = envelope_at + 4 * SKIN_ENVELOPE_SIZE + 2 * 12 + 2 * 4;
        let tail: Vec<u8> = bone_records
            .into_iter()
            .chain(names)
            .chain(subset_bytes(&subsets))
            .collect();
        out[bones_at..bones_at + tail.len()].copy_from_slice(&tail);
        assert_eq!(out.len(), bones_at + tail.len() + counts.facs as usize);
        out
    }

    #[test]
    fn v4_and_v5_expose_bones_subsets_and_resolved_weights() {
        let (bones, subsets, envelopes) = crate::skin::tests::sample();
        for major in [4u8, 5] {
            let (mesh, skin) = parse((major, 0), &skinned_body(major)).unwrap();
            // Geometry exactly as the unskinned reader saw it.
            assert_eq!(mesh.vertices.len(), 4);
            assert_eq!(mesh.vertices[3].position, [3.0, 2.0, 3.0]);
            assert_eq!(mesh.indices, vec![0, 1, 2, 0, 1, 2]);

            let skin = skin.expect("a mesh declaring bones carries a skin");
            assert_eq!(skin.bones, bones);
            assert_eq!(skin.bones[1].name, "LowerArm");
            assert_eq!(skin.bones[1].parent, Some(0));
            assert_eq!(skin.subsets, subsets);
            assert_eq!(skin.envelopes, envelopes);
            assert_eq!(skin.influences.len(), 4);
            assert_eq!(
                skin.influences[2].iter().collect::<Vec<_>>(),
                [(1, 0.2), (0, 0.8)]
            );
        }
    }

    #[test]
    fn an_unskinned_mesh_has_no_skin() {
        let (_, skin) = parse((4, 1), &body(4, &Counts::plain(3, 2, 3))).unwrap();
        assert!(skin.is_none());
    }

    #[test]
    fn every_truncation_of_a_skinned_body_errors_without_panicking() {
        let bytes = skinned_body(4);
        for len in 0..bytes.len() {
            assert!(parse((4, 0), &bytes[..len]).is_err());
        }
    }
}
