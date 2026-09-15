//! Decoder for the UniqueId property type.

use rbx_dom::{UniqueId, Variant};

use super::PropValues;
use crate::codec::{rotated_i64, Reader};
use crate::error::BinaryError;

/// Reads a UniqueId property array.
///
/// The interleaving unit is the whole 16-byte struct, not its individual fields:
/// an array of two values stores byte 0 of both, then byte 1 of both, and so on.
/// Reading it as three separate interleaved blocks (u32, u32, i64) would silently
/// scramble every array of more than one instance.
pub(super) fn unique_ids(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(reader
        .interleaved_bytes::<16>(count)?
        .into_iter()
        .map(|record| {
            let word = |offset: usize| {
                u32::from_be_bytes([
                    record[offset],
                    record[offset + 1],
                    record[offset + 2],
                    record[offset + 3],
                ])
            };
            // `random` is bit-rotated, not zigzag encoded: Roblox reuses the float
            // trick here to keep the sign bit out of the high interleaved column.
            let random = rotated_i64(u64::from(word(8)) << 32 | u64::from(word(12)));

            Some(Variant::UniqueId(UniqueId {
                index: word(0),
                time: word(4),
                random,
            }))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Workspace.UniqueId from TestPlace.rbxl. `time` counts seconds since
    // 2021-01-01, so 0x09e550b2 is 2026-04-06, the day the place was created.
    #[test]
    fn unique_id_reads_the_real_workspace_identifier() {
        let payload = [
            0x00, 0x00, 0x00, 0x02, 0x09, 0xe5, 0x50, 0xb2, 0x00, 0x70, 0x5b, 0x7d, 0x33, 0x4c,
            0x78, 0x6a,
        ];

        let values = unique_ids(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::UniqueId(UniqueId {
                index: 2,
                time: 0x09e5_50b2,
                random: 0x0038_2dbe_99a6_3c35,
            }))
        );
    }

    // ConfigureServerService.UniqueId from TestPlace.rbxl: the only fixture value
    // whose top bit is set, which is what the rotation is there to move.
    #[test]
    fn rotation_keeps_a_high_bit_random_positive() {
        let payload = [
            0x00, 0x00, 0x03, 0x7d, 0x09, 0xe5, 0x50, 0x48, 0x91, 0xd7, 0x0a, 0xd3, 0x28, 0xb0,
            0x66, 0x00,
        ];

        let values = unique_ids(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::UniqueId(UniqueId {
                index: 0x37d,
                time: 0x09e5_5048,
                random: 0x48eb_8569_9458_3300,
            }))
        );
    }

    #[test]
    fn an_array_interleaves_whole_sixteen_byte_records() {
        // Two records, 0x00..0x0F and 0x10..0x1F, stored column by column.
        let payload: Vec<u8> = (0..16u8).flat_map(|i| [i, i + 0x10]).collect();

        let values = unique_ids(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::UniqueId(UniqueId {
                index: 0x0001_0203,
                time: 0x0405_0607,
                random: rotated_i64(0x0809_0a0b_0c0d_0e0f),
            }))
        );
        assert_eq!(
            values[1],
            Some(Variant::UniqueId(UniqueId {
                index: 0x1011_1213,
                time: 0x1415_1617,
                random: rotated_i64(0x1819_1a1b_1c1d_1e1f),
            }))
        );
    }

    #[test]
    fn a_truncated_record_is_an_error() {
        assert!(unique_ids(&mut Reader::new(&[0; 15]), 1).is_err());
    }
}
