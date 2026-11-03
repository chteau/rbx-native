//! Chunk-level decompression and iteration.
//!
//! After the file header, the binary format consists of a sequence of chunks.
//! Each chunk has a 16-byte header and may be compressed with LZ4 or Zstandard.

use std::io::Read;

use crate::error::BinaryError;

const CHUNK_HEADER_LEN: usize = 16;
// Zstandard frame magic; distinguishes a Zstd payload from a raw LZ4 block
// since the chunk header carries no explicit codec tag.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
// Ceiling on what one chunk may claim to decompress to. Real chunks top out
// at a few hundred MB for huge places; 1 GiB leaves headroom while keeping a
// forged header from asking for gigabytes.
const MAX_UNCOMPRESSED_LEN: usize = 1 << 30;
// An LZ4 block can't expand more than ~255x, so a larger claim is a lie.
const LZ4_MAX_RATIO: usize = 255;

/// A decompressed chunk from the binary file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub name: [u8; 4],
    pub data: Vec<u8>,
}

impl Chunk {
    /// Returns the chunk name as a string, trimmed at the first NUL byte.
    pub fn name_str(&self) -> &str {
        let end = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// Iterator over chunks in a binary file.
///
/// Stops at the "END\0" marker chunk, which signals the end of the file.
pub struct ChunkReader<'a> {
    remaining: &'a [u8],
    done: bool,
}

/// Creates a chunk reader from the body of a binary file (after the header).
pub fn read_chunks(input: &[u8]) -> ChunkReader<'_> {
    ChunkReader {
        remaining: input,
        done: false,
    }
}

impl<'a> ChunkReader<'a> {
    fn parse_next(&mut self) -> Result<Option<Chunk>, BinaryError> {
        if self.remaining.is_empty() {
            return Ok(None);
        }
        if self.remaining.len() < CHUNK_HEADER_LEN {
            return Err(BinaryError::ChunkHeaderTruncated(self.remaining.len()));
        }

        let name: [u8; 4] = self.remaining[0..4].try_into().unwrap();
        let compressed_len = u32::from_le_bytes(self.remaining[4..8].try_into().unwrap()) as usize;
        let uncompressed_len =
            u32::from_le_bytes(self.remaining[8..12].try_into().unwrap()) as usize;
        // bytes 12..16 are reserved and always zero.

        let payload_len = if compressed_len == 0 {
            uncompressed_len
        } else {
            compressed_len
        };
        let body_end = CHUNK_HEADER_LEN
            .checked_add(payload_len)
            .ok_or(BinaryError::ChunkTooLarge)?;

        if self.remaining.len() < body_end {
            return Err(BinaryError::ChunkTruncated {
                name: String::from_utf8_lossy(&name)
                    .trim_end_matches('\0')
                    .to_string(),
                declared: payload_len,
                available: self.remaining.len() - CHUNK_HEADER_LEN,
            });
        }

        let payload = &self.remaining[CHUNK_HEADER_LEN..body_end];
        self.remaining = &self.remaining[body_end..];

        let data = if compressed_len == 0 {
            payload.to_vec()
        } else {
            decompress(payload, uncompressed_len)?
        };

        Ok(Some(Chunk { name, data }))
    }
}

impl<'a> Iterator for ChunkReader<'a> {
    type Item = Result<Chunk, BinaryError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        match self.parse_next() {
            Ok(Some(chunk)) => {
                if chunk.name == *b"END\0" {
                    self.done = true;
                }
                Some(Ok(chunk))
            }
            Ok(None) => {
                self.done = true;
                None
            }
            Err(err) => {
                self.done = true;
                Some(Err(err))
            }
        }
    }
}

fn decompress(payload: &[u8], uncompressed_len: usize) -> Result<Vec<u8>, BinaryError> {
    if uncompressed_len > MAX_UNCOMPRESSED_LEN {
        return Err(BinaryError::DecompressedTooLarge(uncompressed_len));
    }
    if payload.starts_with(&ZSTD_MAGIC) {
        decompress_zstd(payload, uncompressed_len)
    } else {
        decompress_lz4(payload, uncompressed_len)
    }
}

fn decompress_lz4(payload: &[u8], uncompressed_len: usize) -> Result<Vec<u8>, BinaryError> {
    if uncompressed_len > payload.len().saturating_mul(LZ4_MAX_RATIO) {
        return Err(BinaryError::DecompressedTooLarge(uncompressed_len));
    }
    lz4_flex::block::decompress(payload, uncompressed_len)
        .map_err(|err| BinaryError::Lz4Decompression(err.to_string()))
}

fn decompress_zstd(payload: &[u8], uncompressed_len: usize) -> Result<Vec<u8>, BinaryError> {
    let decoder = ruzstd::decoding::StreamingDecoder::new(payload)
        .map_err(|err| BinaryError::ZstdDecompression(err.to_string()))?;
    // One byte past the declared size is enough to tell the frame overruns it.
    let mut limited = decoder.take(uncompressed_len as u64 + 1);
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .map_err(|err| BinaryError::ZstdDecompression(err.to_string()))?;
    if out.len() > uncompressed_len {
        return Err(BinaryError::DecompressedTooLarge(out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::parse_header;

    const FPS: &[u8] = include_bytes!("../../../assets/tests/FPS.rbxm");
    const TEST_PLACE: &[u8] = include_bytes!("../../../assets/tests/TestPlace.rbxl");

    #[test]
    fn fps_first_chunk_is_meta() {
        let (_, rest) = parse_header(FPS).unwrap();
        let chunk = read_chunks(rest).next().unwrap().unwrap();

        assert_eq!(chunk.name_str(), "META");
        assert_eq!(chunk.data.len(), 34);
    }

    #[test]
    fn test_place_sstr_chunk_is_zstd_and_decodes() {
        let (_, rest) = parse_header(TEST_PLACE).unwrap();
        let chunk = read_chunks(rest).next().unwrap().unwrap();

        assert_eq!(chunk.name_str(), "SSTR");
        assert_eq!(chunk.data.len(), 28);
    }

    #[test]
    fn fps_iteration_never_panics_and_ends_on_end_marker() {
        let (_, rest) = parse_header(FPS).unwrap();
        let chunks: Vec<Chunk> = read_chunks(rest).collect::<Result<_, _>>().unwrap();

        assert_eq!(chunks.last().unwrap().name_str(), "END");
    }

    #[test]
    fn test_place_iteration_never_panics_and_ends_on_end_marker() {
        let (_, rest) = parse_header(TEST_PLACE).unwrap();
        let chunks: Vec<Chunk> = read_chunks(rest).collect::<Result<_, _>>().unwrap();

        assert_eq!(chunks.last().unwrap().name_str(), "END");
    }

    fn chunk_bytes(compressed_len: u32, uncompressed_len: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = b"INST".to_vec();
        out.extend(compressed_len.to_le_bytes());
        out.extend(uncompressed_len.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(payload);
        out
    }

    fn first_err(bytes: &[u8]) -> BinaryError {
        read_chunks(bytes).next().unwrap().unwrap_err()
    }

    #[test]
    fn lz4_chunk_declaring_gigabytes_is_rejected_without_allocating() {
        let payload = [0u8; 8];
        let bytes = chunk_bytes(payload.len() as u32, u32::MAX, &payload);
        assert!(matches!(
            first_err(&bytes),
            BinaryError::DecompressedTooLarge(_)
        ));
    }

    #[test]
    fn lz4_chunk_claiming_impossible_ratio_is_rejected() {
        let payload = [0u8; 8];
        let bytes = chunk_bytes(payload.len() as u32, 1 << 20, &payload);
        assert!(matches!(
            first_err(&bytes),
            BinaryError::DecompressedTooLarge(_)
        ));
    }

    #[test]
    fn zstd_frame_larger_than_declared_is_rejected() {
        let payload = ruzstd::encoding::compress_to_vec(
            &vec![0u8; 1 << 16][..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        assert!(payload.starts_with(&ZSTD_MAGIC));
        let bytes = chunk_bytes(payload.len() as u32, 16, &payload);
        assert!(matches!(
            first_err(&bytes),
            BinaryError::DecompressedTooLarge(_)
        ));
    }

    #[test]
    fn zstd_chunk_declaring_over_the_ceiling_is_rejected() {
        let payload = ruzstd::encoding::compress_to_vec(
            &[1u8, 2, 3][..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let bytes = chunk_bytes(payload.len() as u32, u32::MAX, &payload);
        assert!(matches!(
            first_err(&bytes),
            BinaryError::DecompressedTooLarge(_)
        ));
    }

    #[test]
    fn honest_zstd_chunk_still_decodes() {
        let data = vec![7u8; 4096];
        let payload = ruzstd::encoding::compress_to_vec(
            &data[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let bytes = chunk_bytes(payload.len() as u32, data.len() as u32, &payload);
        let chunk = read_chunks(&bytes).next().unwrap().unwrap();
        assert_eq!(chunk.data, data);
    }

    #[test]
    fn truncated_chunk_error_describes_the_file() {
        let bytes = chunk_bytes(0, 100, &[0u8; 30]);
        let err = first_err(&bytes);
        assert!(matches!(
            err,
            BinaryError::ChunkTruncated {
                declared: 100,
                available: 30,
                ..
            }
        ));
        assert_eq!(
            err.to_string(),
            "the file is cut off: chunk 'INST' declares 100 bytes of data but only 30 are left in the file"
        );
    }

    #[test]
    fn cut_off_chunk_header_says_the_file_is_cut_off() {
        let bytes = chunk_bytes(0, 4, &[0u8; 4]);
        let err = first_err(&bytes[..8]);
        assert_eq!(
            err.to_string(),
            "the file is cut off: a chunk header needs 16 bytes but only 8 are left in the file"
        );
    }
}
