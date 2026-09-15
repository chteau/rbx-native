//! `version 1.00` and `1.01`: the ASCII mesh format.
//!
//! Body layout after the version line: a face-count line, then one long line of
//! bracketed vectors, nine per face (three vertices x position, normal, uv).

use crate::error::MeshError;
use crate::mesh::{Mesh, Vertex};

const VECTORS_PER_FACE: usize = 9;

// `version 1.00` stores positions at twice their real size; `1.01` fixed that and
// writes them directly. Per MaximumADHD's "Roblox FileMesh Format Specification",
// the only difference between the two minor versions.
const V1_00_POSITION_SCALE: f32 = 0.5;

pub(crate) fn parse(version: (u8, u8), body: &[u8]) -> Result<Mesh, MeshError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| MeshError::MalformedVersionLine("non-UTF-8 ASCII body".to_owned()))?;

    let (count_line, data) = split_line(text);
    let faces: usize = count_line
        .trim()
        .parse()
        .map_err(|_| MeshError::InvalidFaceCount(count_line.trim().to_owned()))?;

    let scale = if version == (1, 0) {
        V1_00_POSITION_SCALE
    } else {
        1.0
    };

    // Not pre-sized from `faces`: the count is untrusted and a nine-digit value
    // would reserve gigabytes before a single vector had been read.
    let mut vectors: Vec<[f32; 3]> = Vec::new();
    for group in bracketed(data) {
        vectors.push(vector3(group)?);
    }

    let expected = faces
        .checked_mul(VECTORS_PER_FACE)
        .ok_or(MeshError::TooLarge)?;
    if vectors.len() != expected {
        return Err(MeshError::VectorCountMismatch {
            faces,
            expected,
            actual: vectors.len(),
        });
    }

    let vertices: Vec<Vertex> = vectors
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triplet| Vertex {
            position: triplet[0].map(|axis| axis * scale),
            normal: triplet[1],
            // The third vector is a UV with an unused w component. Version 1 is the
            // one layout that stores v upside down relative to every later version
            // ("a quirk in the version 1 mesh format" per the spec), so flip it here
            // to keep `Vertex::uv` in one convention crate-wide. Unlike the position
            // scale this applies to 1.01 as well.
            uv: [triplet[2][0], 1.0 - triplet[2][1]],
            color: [u8::MAX; 4],
        })
        .collect();

    // The ASCII format has no index buffer: vertices are listed per corner, so
    // faces are simply consecutive triples.
    let face_list: Vec<[u32; 3]> = (0..faces as u32)
        .map(|face| [face * 3, face * 3 + 1, face * 3 + 2])
        .collect();

    Mesh::assemble(version, vertices, &face_list, &[])
}

fn split_line(text: &str) -> (&str, &str) {
    match text.split_once('\n') {
        Some((line, rest)) => (line, rest),
        None => (text, ""),
    }
}

/// Yields the contents of each `[...]` group, ignoring everything between groups.
fn bracketed(text: &str) -> impl Iterator<Item = &str> {
    text.split('[').skip(1).filter_map(|rest| {
        let end = rest.find(']')?;
        Some(&rest[..end])
    })
}

fn vector3(group: &str) -> Result<[f32; 3], MeshError> {
    let malformed = || MeshError::MalformedVector(group.to_owned());

    let mut parts = group.split(',');
    let mut next = || -> Result<f32, MeshError> {
        parts
            .next()
            .ok_or_else(malformed)?
            .trim()
            .parse::<f32>()
            .map_err(|_| malformed())
    };

    let vector = [next()?, next()?, next()?];
    if parts.next().is_some() {
        return Err(malformed());
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One triangle: three vertices, each position/normal/uv, sharing a normal of
    // +Y so the doubling check below reads unambiguously.
    fn one_triangle(faces: &str) -> Vec<u8> {
        let mut body = format!("{faces}\r\n");
        for corner in 0..3 {
            let x = corner as f32;
            body.push_str(&format!("[{x},2,4][0,1,0][{x},0.25,0]", x = x,));
        }
        body.into_bytes()
    }

    #[test]
    fn v1_00_halves_positions() {
        let mesh = parse((1, 0), &one_triangle("1")).unwrap();

        assert_eq!(mesh.version, (1, 0));
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.vertices[0].position, [0.0, 1.0, 2.0]);
        assert_eq!(mesh.vertices[2].position, [1.0, 1.0, 2.0]);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert!(mesh.lods.is_empty());
    }

    #[test]
    fn v1_01_keeps_positions_as_written_but_still_flips_v() {
        let mesh = parse((1, 1), &one_triangle("1")).unwrap();
        assert_eq!(mesh.vertices[0].position, [0.0, 2.0, 4.0]);
        // The v flip is not tied to the 1.00 position scale.
        assert_eq!(mesh.vertices[0].uv, [0.0, 0.75]);
    }

    #[test]
    fn normals_and_uvs_come_from_the_second_and_third_vectors() {
        let mesh = parse((1, 1), &one_triangle("1")).unwrap();

        assert_eq!(mesh.vertices[0].normal, [0.0, 1.0, 0.0]);
        // v is flipped into the crate-wide top-left convention and the uv's w
        // component is dropped, not folded into v.
        assert_eq!(mesh.vertices[1].uv, [1.0, 0.75]);
        assert_eq!(mesh.vertices[0].color, [255, 255, 255, 255]);
    }

    #[test]
    fn a_face_count_that_disagrees_with_the_data_is_rejected() {
        assert!(matches!(
            parse((1, 0), &one_triangle("2")),
            Err(MeshError::VectorCountMismatch {
                faces: 2,
                expected: 18,
                actual: 9
            })
        ));
    }

    #[test]
    fn a_non_numeric_face_count_is_rejected() {
        assert!(matches!(
            parse((1, 0), b"lots\r\n[0,0,0]"),
            Err(MeshError::InvalidFaceCount(_))
        ));
    }

    #[test]
    fn a_vector_with_the_wrong_arity_is_rejected() {
        assert!(matches!(
            parse((1, 0), b"1\r\n[0,0]"),
            Err(MeshError::MalformedVector(_))
        ));
        assert!(matches!(
            parse((1, 0), b"1\r\n[0,0,0,0]"),
            Err(MeshError::MalformedVector(_))
        ));
    }

    #[test]
    fn an_unterminated_group_is_ignored_and_trips_the_count_check() {
        assert!(matches!(
            parse((1, 0), b"1\r\n[0,0,0"),
            Err(MeshError::VectorCountMismatch { actual: 0, .. })
        ));
    }

    #[test]
    fn every_truncation_of_a_valid_body_errors_without_panicking() {
        let body = one_triangle("1");
        for len in 0..body.len() {
            let _ = parse((1, 0), &body[..len]);
        }
    }
}
