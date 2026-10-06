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

/// The closing `END\0` chunk exactly as Roblox writes it: uncompressed, holding
/// `</roblox>`. An LZ4 block of an empty payload (one zero byte claiming zero
/// output) is not something Roblox's own reader accepts.
pub(crate) fn write_end() -> Vec<u8> {
    const BODY: &[u8] = b"</roblox>";
    let mut out = Vec::with_capacity(16 + BODY.len());
    out.extend_from_slice(b"END\0");
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(BODY.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(BODY);
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
    fn the_end_chunk_is_stored_uncompressed_like_roblox_writes_it() {
        let chunk_bytes = write_end();
        assert_eq!(
            &chunk_bytes[4..8],
            &[0, 0, 0, 0],
            "compressed length 0 = raw"
        );
        let chunk = read_chunks(&chunk_bytes).next().unwrap().unwrap();

        assert_eq!(chunk.name_str(), "END");
        assert_eq!(chunk.data, b"</roblox>");
    }
}
