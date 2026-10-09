//! Parsing for the Roblox `.mesh` geometry format, `version 1.00` through `7.00`.
//!
//! Entry point: [`parse`]. Dispatches on the ASCII version line to the ASCII
//! (v1), early binary (v2/v3), skinnable binary (v4/v5) or chunked (v6/v7)
//! layout and returns a single [`Mesh`] carrying LOD 0 geometry.

mod chunked;
mod draco;
mod error;
mod header;
mod mesh;
mod reader;
mod records;
mod v1;
mod v2;
mod v4;

pub use error::MeshError;
pub use mesh::{Aabb, Mesh, Vertex};

/// Parses a Roblox mesh file.
///
/// Any major version past 7 is rejected rather than guessed at: a new layout
/// read as an old one yields plausible garbage instead of an error.
pub fn parse(bytes: &[u8]) -> Result<Mesh, MeshError> {
    let (version, body) = header::parse_version(bytes)?;

    match version.0 {
        1 => v1::parse(version, body),
        2 | 3 => v2::parse(version, body),
        4 | 5 => v4::parse(version, body),
        6 | 7 => chunked::parse(version, body),
        _ => Err(MeshError::UnsupportedVersion {
            major: version.0,
            minor: version.1,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_on_the_version_line() {
        // A v2 header behind a v1 version line must be read as ASCII and fail
        // there, proving dispatch follows the text and not the bytes.
        assert!(matches!(
            parse(b"version 1.00\r\n\x0c\x00(\x0c"),
            Err(MeshError::InvalidFaceCount(_))
        ));
    }

    #[test]
    fn rejects_versions_with_no_known_layout() {
        assert!(matches!(
            parse(b"version 8.00\n"),
            Err(MeshError::UnsupportedVersion { major: 8, .. })
        ));
        assert!(matches!(
            parse(b"version 7.00\n"),
            Err(MeshError::MissingCoreMesh)
        ));
    }

    #[test]
    fn rejects_input_that_is_not_a_mesh() {
        assert!(matches!(parse(b""), Err(MeshError::MissingVersionLine)));
        assert!(matches!(
            parse(b"<roblox!\x89\xff\x0d\x0a\x1a\x0a"),
            Err(MeshError::MissingVersionLine)
        ));
    }

    #[test]
    fn every_truncation_and_mutation_of_a_minimal_mesh_is_safe() {
        let mut body = Vec::from(b"version 2.00\n");
        body.extend_from_slice(&12u16.to_le_bytes());
        body.extend_from_slice(&[40, 12]);
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&1u32.to_le_bytes());
        body.extend(std::iter::repeat_n(0u8, 3 * 40));
        for index in 0u32..3 {
            body.extend_from_slice(&index.to_le_bytes());
        }
        assert!(parse(&body).is_ok());

        for len in 0..body.len() {
            let _ = parse(&body[..len]);
        }

        // xorshift* keeps the mutation deterministic without a dev-dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..400 {
            let mut damaged = body.clone();
            for _ in 0..8 {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let index =
                    (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize % damaged.len();
                damaged[index] = (state >> 8) as u8;
            }
            let _ = parse(&damaged);
        }
    }
}
