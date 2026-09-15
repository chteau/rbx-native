//! INST (instance) chunk parsing.
//!
//! One INST chunk per class, declaring the referent IDs of every instance of that class
//! in the order that PROP chunks use when assigning properties.

use crate::codec::Reader;
use crate::error::BinaryError;

/// Parsed content of an INST chunk.
pub(crate) struct InstChunk {
    pub(crate) class_id: i32,
    pub(crate) class_name: String,
    pub(crate) referents: Vec<i32>,
}

/// Parses an INST chunk payload.
pub(crate) fn parse(data: &[u8]) -> Result<InstChunk, BinaryError> {
    let mut reader = Reader::new(data);

    let class_id = reader.i32()?;
    let class_name = reader.sized_name()?;
    let is_service = reader.u8()? != 0;
    let count = reader.length()?;
    let referents = reader.referents(count)?;

    // Service classes carry one extra byte per instance (observed as 0x01 in
    // TestPlace.rbxl) telling whether the service is rooted in this file.
    // Nothing in the DOM consumes it yet, so it is only skipped to stay aligned.
    if is_service {
        reader.take(count)?;
    }

    Ok(InstChunk {
        class_id,
        class_name,
        referents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_non_service_class() {
        let mut data = Vec::new();
        data.extend_from_slice(&7i32.to_le_bytes());
        data.extend_from_slice(&4i32.to_le_bytes());
        data.extend_from_slice(b"Part");
        data.push(0);
        data.extend_from_slice(&2i32.to_le_bytes());
        // Referents 5 then 6: zigzag(10) = 5, zigzag(2) = 1 as a delta.
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 10, 2]);

        let chunk = parse(&data).unwrap();

        assert_eq!(chunk.class_id, 7);
        assert_eq!(chunk.class_name, "Part");
        assert_eq!(chunk.referents, vec![5, 6]);
    }

    #[test]
    fn service_flag_consumes_the_trailing_byte_array() {
        let mut data = Vec::new();
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&9i32.to_le_bytes());
        data.extend_from_slice(b"Workspace");
        data.push(1);
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&[0, 0, 0, 2]);
        data.push(1);

        let chunk = parse(&data).unwrap();

        assert_eq!(chunk.referents, vec![1]);
    }

    #[test]
    fn truncated_chunk_errors() {
        let data = [0u8; 6];
        assert!(parse(&data).is_err());
    }
}
