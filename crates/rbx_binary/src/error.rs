//! Error types for the binary format parser.

use thiserror::Error;

// Third-party decoder error types aren't part of this crate's public API
// (façade rule); their messages are captured as strings instead.
/// Errors that can occur while parsing a Roblox binary file.
#[derive(Debug, Error)]
pub enum BinaryError {
    #[error("input too short: expected at least {expected} bytes, got {actual}")]
    TooShort { expected: usize, actual: usize },

    #[error("invalid magic header")]
    InvalidMagic,

    #[error("invalid signature bytes")]
    InvalidSignature,

    #[error("chunk payload length overflows the address space")]
    ChunkTooLarge,

    #[error(
        "the file is cut off: a chunk header needs 16 bytes but only {0} are left in the file"
    )]
    ChunkHeaderTruncated(usize),

    #[error(
        "the file is cut off: chunk '{name}' declares {declared} bytes of data but only {available} are left in the file"
    )]
    ChunkTruncated {
        name: String,
        declared: usize,
        available: usize,
    },

    #[error("chunk claims to decompress to {0} bytes, which is more than it can hold")]
    DecompressedTooLarge(usize),

    #[error("failed to decompress LZ4 block: {0}")]
    Lz4Decompression(String),

    #[error("failed to decompress Zstd frame: {0}")]
    ZstdDecompression(String),

    #[error("length field is negative or does not fit in memory: {0}")]
    InvalidLength(i32),

    #[error("property type id {0:#04x} has no decoder")]
    UnsupportedPropertyType(u8),

    #[error("expected the inner type id {expected:#04x}, got {actual:#04x}")]
    UnexpectedInnerType { expected: u8, actual: u8 },

    #[error("content source type {0} is not one of None, Uri or Object")]
    UnknownContentSource(u32),

    #[error("more content values claim a uri or a referent than the chunk carries")]
    ContentPoolExhausted,
}
