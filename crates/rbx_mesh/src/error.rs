//! Error types for the mesh format parser.

use thiserror::Error;

/// Errors that can occur while parsing a Roblox `.mesh` file.
#[derive(Debug, Error)]
pub enum MeshError {
    #[error("input too short: expected at least {expected} bytes, got {actual}")]
    TooShort { expected: usize, actual: usize },

    #[error("no `version X.YY` line at the start of the file")]
    MissingVersionLine,

    #[error("malformed version line: {0:?}")]
    MalformedVersionLine(String),

    #[error("mesh version {major}.{minor:02} is not supported")]
    UnsupportedVersion { major: u8, minor: u8 },

    #[error("header claims {actual} bytes, this version requires {expected}")]
    UnexpectedHeaderSize { expected: u16, actual: u16 },

    #[error("vertex stride {0} is neither 36 (no color) nor 40 (with color)")]
    UnsupportedVertexSize(u8),

    #[error("face stride {0} is not 12 (three 32-bit indices)")]
    UnsupportedFaceSize(u8),

    #[error("declared counts overflow the address space")]
    TooLarge,

    #[error("expected a decimal integer on the face-count line, got {0:?}")]
    InvalidFaceCount(String),

    #[error("expected {expected} bracketed vectors for {faces} faces, got {actual}")]
    VectorCountMismatch {
        faces: usize,
        expected: usize,
        actual: usize,
    },

    #[error("malformed vector {0:?}: expected three comma-separated floats")]
    MalformedVector(String),

    #[error("face index {index} is out of range for {vertex_count} vertices")]
    IndexOutOfRange { index: u32, vertex_count: usize },

    #[error("{0} bytes left over after the last declared block")]
    TrailingBytes(usize),
}
