//! Chunk compression and framing, the encode counterpart of `crate::chunk::read_chunks`.

/// Wraps `payload` in a 16-byte chunk header and LZ4-compresses the body.
///
/// Layout matches what `chunk::ChunkReader` parses: 4-byte name, compressed length,
/// uncompressed length, 4 reserved zero bytes, then the compressed payload. The reader
/// auto-detects the codec from the payload's own magic bytes, so a plain LZ4 block
/// (no Zstd frame header) is always read correctly regardless of the encoder used here.
pub(crate) fn write_chunk(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let compressed = lz4_flex::block::compress(payload);

    let mut out = Vec::with_capacity(16 + compressed.len());
    out.extend_from_slice(name);
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&compressed);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::read_chunks;

    #[test]
    fn a_written_chunk_round_trips_through_the_reader() {
        let payload = b"hello binary format".repeat(4);
        let chunk_bytes = write_chunk(b"TEST", &payload);

        let chunk = read_chunks(&chunk_bytes).next().unwrap().unwrap();

        assert_eq!(chunk.name_str(), "TEST");
        assert_eq!(chunk.data, payload);
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let chunk_bytes = write_chunk(b"END\0", &[]);
        let chunk = read_chunks(&chunk_bytes).next().unwrap().unwrap();

        assert_eq!(chunk.name_str(), "END");
        assert!(chunk.data.is_empty());
    }
}
