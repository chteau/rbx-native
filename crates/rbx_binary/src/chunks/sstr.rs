//! Shared string table parsing.
//!
//! SharedString properties reference a deduplicated string table stored in the SSTR chunk.
//! Entries may contain arbitrary binary data (mesh data, serialized attributes).

use crate::codec::Reader;
use crate::error::BinaryError;

const MD5_LEN: usize = 16;

/// Parses the shared string table from a SSTR chunk payload.
///
/// Each entry is prefixed with an MD5 hash (which readers ignore) followed by the string
/// data itself. Returns a vector where the index is used by SharedString properties.
pub(crate) fn parse(data: &[u8]) -> Result<Vec<Vec<u8>>, BinaryError> {
    let mut reader = Reader::new(data);

    let _version = reader.i32()?;
    let count = reader.length()?;

    (0..count)
        .map(|_| {
            // The MD5 hash of the entry is only used by Roblox to deduplicate
            // on write; readers can ignore it.
            reader.take(MD5_LEN)?;
            Ok(reader.sized_bytes()?.to_vec())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_skips_hashes() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&[0xAB; MD5_LEN]);
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(b"abc");

        assert_eq!(parse(&data).unwrap(), vec![b"abc".to_vec()]);
    }

    #[test]
    fn truncated_table_errors() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        assert!(parse(&data).is_err());
    }
}
