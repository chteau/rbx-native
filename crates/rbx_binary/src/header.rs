//! File header parsing for Roblox binary files.
//!
//! Every .rbxm/.rbxl file begins with a 32-byte header containing magic bytes,
//! signature, version, and instance/type counts.

use crate::error::BinaryError;

const MAGIC: &[u8; 8] = b"<roblox!";
const SIGNATURE: [u8; 6] = [0x89, 0xFF, 0x0D, 0x0A, 0x1A, 0x0A];
const HEADER_LEN: usize = 32;

/// Parsed file header information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileHeader {
    pub version: u16,
    pub num_types: i32,
    pub num_instances: i32,
}

/// Parses a file header from the start of input and returns the header plus remaining bytes.
///
/// Layout (32 bytes total): 8B magic, 6B signature, 2B version LE,
/// 4B num_types LE, 4B num_instances LE, 8B reserved (always zero).
pub fn parse_header(input: &[u8]) -> Result<(FileHeader, &[u8]), BinaryError> {
    if input.len() < HEADER_LEN {
        return Err(BinaryError::TooShort {
            expected: HEADER_LEN,
            actual: input.len(),
        });
    }

    if &input[0..8] != MAGIC {
        return Err(BinaryError::InvalidMagic);
    }
    if input[8..14] != SIGNATURE {
        return Err(BinaryError::InvalidSignature);
    }

    let version = u16::from_le_bytes(input[14..16].try_into().unwrap());
    let num_types = i32::from_le_bytes(input[16..20].try_into().unwrap());
    let num_instances = i32::from_le_bytes(input[20..24].try_into().unwrap());

    let header = FileHeader {
        version,
        num_types,
        num_instances,
    };
    Ok((header, &input[HEADER_LEN..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: &[u8] = include_bytes!("../../../assets/tests/FPS.rbxm");
    const TEST_PLACE: &[u8] = include_bytes!("../../../assets/tests/TestPlace.rbxl");

    #[test]
    fn parses_fps_rbxm_header() {
        let (header, rest) = parse_header(FPS).unwrap();
        assert_eq!(header.version, 0);
        assert_eq!(header.num_types, 10);
        assert_eq!(header.num_instances, 48);
        assert_eq!(rest.len(), FPS.len() - HEADER_LEN);
    }

    #[test]
    fn parses_test_place_rbxl_header() {
        let (header, rest) = parse_header(TEST_PLACE).unwrap();
        assert_eq!(header.version, 0);
        assert_eq!(header.num_types, 81);
        assert_eq!(header.num_instances, 81);
        assert_eq!(rest.len(), TEST_PLACE.len() - HEADER_LEN);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut bytes = FPS.to_vec();
        bytes[0] = b'X';
        assert!(matches!(
            parse_header(&bytes),
            Err(BinaryError::InvalidMagic)
        ));
    }

    #[test]
    fn rejects_invalid_signature() {
        let mut bytes = FPS.to_vec();
        bytes[8] = 0x00;
        assert!(matches!(
            parse_header(&bytes),
            Err(BinaryError::InvalidSignature)
        ));
    }

    #[test]
    fn rejects_too_short_input() {
        assert!(matches!(
            parse_header(&FPS[..16]),
            Err(BinaryError::TooShort {
                expected: HEADER_LEN,
                actual: 16
            })
        ));
    }
}
