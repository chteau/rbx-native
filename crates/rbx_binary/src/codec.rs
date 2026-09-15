//! Binary codec primitives for reading and decoding Roblox .rbxm/.rbxl format chunks.
//!
//! Handles bounds-checked parsing, compression-related transformations (zigzag, bit rotation),
//! and the interleaved column layout used for floats and referent arrays.

use crate::error::BinaryError;

// Sequential reader over one already-decompressed chunk payload. Every method
// is bounds-checked so a truncated or hostile file yields an error instead of
// a panic.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], BinaryError> {
        if self.remaining() < len {
            return Err(BinaryError::TooShort {
                expected: len,
                actual: self.remaining(),
            });
        }
        let out = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, BinaryError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, BinaryError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn i16(&mut self) -> Result<i16, BinaryError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, BinaryError> {
        // Slice length is guaranteed by take(), so try_into cannot fail.
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, BinaryError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, BinaryError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    // Counts and string lengths are stored as i32 even though negative values
    // are meaningless; reject them here rather than wrapping into a huge usize.
    pub(crate) fn length(&mut self) -> Result<usize, BinaryError> {
        let raw = self.i32()?;
        usize::try_from(raw).map_err(|_| BinaryError::InvalidLength(raw))
    }

    pub(crate) fn sized_bytes(&mut self) -> Result<&'a [u8], BinaryError> {
        let len = self.length()?;
        self.take(len)
    }

    // Class and property names are ASCII in practice; a lossy conversion keeps
    // the parser alive on a file that disagrees.
    pub(crate) fn sized_name(&mut self) -> Result<String, BinaryError> {
        Ok(String::from_utf8_lossy(self.sized_bytes()?).into_owned())
    }

    /// Reads `count` fixed-size records stored in column-major (interleaved) layout.
    ///
    /// The format stores bytes column-by-column: all first bytes of the count values,
    /// then all second bytes, etc. This layout enables compression of exponent bits
    /// in floating-point arrays and index fields in referent arrays.
    ///
    /// `WIDTH` is the size of one record; it is not always a machine integer width
    /// (`UniqueId` interleaves whole 16-byte structs).
    pub(crate) fn interleaved_bytes<const WIDTH: usize>(
        &mut self,
        count: usize,
    ) -> Result<Vec<[u8; WIDTH]>, BinaryError> {
        let payload = self.take(byte_len(count, WIDTH)?)?;
        Ok((0..count)
            .map(|i| std::array::from_fn(|column| payload[column * count + i]))
            .collect())
    }

    /// Reads `count` big-endian 32-bit unsigned integers in interleaved layout.
    pub(crate) fn interleaved_u32(&mut self, count: usize) -> Result<Vec<u32>, BinaryError> {
        Ok(self
            .interleaved_bytes::<4>(count)?
            .into_iter()
            .map(u32::from_be_bytes)
            .collect())
    }

    /// Reads `count` big-endian 64-bit unsigned integers in interleaved layout.
    pub(crate) fn interleaved_u64(&mut self, count: usize) -> Result<Vec<u64>, BinaryError> {
        Ok(self
            .interleaved_bytes::<8>(count)?
            .into_iter()
            .map(u64::from_be_bytes)
            .collect())
    }

    /// Reads `count` 32-bit floats stored in interleaved layout with bit rotation applied.
    ///
    /// Each value is read via [`interleaved_u32`] and then decoded with [`rotated_f32`].
    pub(crate) fn interleaved_f32(&mut self, count: usize) -> Result<Vec<f32>, BinaryError> {
        Ok(self
            .interleaved_u32(count)?
            .into_iter()
            .map(rotated_f32)
            .collect())
    }

    /// Reads `count` referent IDs (instance references), zigzag and delta-decoded.
    ///
    /// Referent arrays (INST, PRNT and Ref properties) are both zigzag encoded *and*
    /// delta encoded: sibling instances usually get consecutive IDs, so deltas compress
    /// far better than absolute values. This method applies both decodings in sequence.
    pub(crate) fn referents(&mut self, count: usize) -> Result<Vec<i32>, BinaryError> {
        let mut acc = 0i32;
        Ok(self
            .interleaved_u32(count)?
            .into_iter()
            .map(|raw| {
                acc = acc.wrapping_add(zigzag_i32(raw));
                acc
            })
            .collect())
    }
}

fn byte_len(count: usize, width: usize) -> Result<usize, BinaryError> {
    count.checked_mul(width).ok_or(BinaryError::ChunkTooLarge)
}

/// Decodes a zigzag-encoded unsigned integer to a signed integer.
///
/// Zigzag encoding maps signed integers to unsigned ones so that small absolute
/// values (close to zero, positive or negative) compress well. The 0 maps to 0,
/// positive integers map to even values, and negative integers to odd values.
pub(crate) fn zigzag_i32(raw: u32) -> i32 {
    ((raw >> 1) as i32) ^ -((raw & 1) as i32)
}

/// Decodes a zigzag-encoded unsigned integer to a 64-bit signed integer.
///
/// See [`zigzag_i32`] for the encoding scheme; this is the 64-bit variant.
pub(crate) fn zigzag_i64(raw: u64) -> i64 {
    ((raw >> 1) as i64) ^ -((raw & 1) as i64)
}

/// Decodes a bit-rotated float stored in interleaved columns.
///
/// Floats are stored with their bits rotated left by one, which moves the sign bit
/// next to the mantissa so that the interleaved exponent column stays mostly constant
/// and compresses well. Decoding rotates back to the right.
pub(crate) fn rotated_f32(raw: u32) -> f32 {
    f32::from_bits(raw.rotate_right(1))
}

/// Decodes a bit-rotated 64-bit integer.
///
/// Same trick as [`rotated_f32`] applied to an integer: the sign bit is stored in
/// the low position so the high interleaved columns stay near-constant. Used by the
/// `Random` field of `UniqueId`, which is *not* zigzag encoded like other integers.
pub(crate) fn rotated_i64(raw: u64) -> i64 {
    raw.rotate_right(1) as i64
}

// --- Encode counterparts, used by the serializer to produce the exact byte
// layouts the functions above decode. Kept next to their decoders so a change
// to one transformation is easy to check against its inverse.

/// Encodes a signed integer to its zigzag representation (inverse of [`zigzag_i32`]).
pub(crate) fn zigzag_encode_i32(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)) as u32
}

/// Encodes a 64-bit signed integer to its zigzag representation (inverse of [`zigzag_i64`]).
pub(crate) fn zigzag_encode_i64(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Encodes a float to its bit-rotated wire representation (inverse of [`rotated_f32`]).
pub(crate) fn rotated_f32_bits(value: f32) -> u32 {
    value.to_bits().rotate_left(1)
}

/// Encodes a 64-bit integer to its bit-rotated wire representation (inverse of [`rotated_i64`]).
pub(crate) fn rotated_i64_bits(value: i64) -> u64 {
    (value as u64).rotate_left(1)
}

/// Writes `records` in column-major (interleaved) layout, the inverse of
/// [`Reader::interleaved_bytes`].
pub(crate) fn interleave_bytes<const WIDTH: usize>(records: &[[u8; WIDTH]]) -> Vec<u8> {
    let count = records.len();
    let mut out = vec![0u8; count * WIDTH];
    for (i, record) in records.iter().enumerate() {
        for (column, &byte) in record.iter().enumerate() {
            out[column * count + i] = byte;
        }
    }
    out
}

/// Writes `values` as interleaved big-endian 32-bit words, the inverse of
/// [`Reader::interleaved_u32`].
pub(crate) fn interleave_u32(values: &[u32]) -> Vec<u8> {
    let records: Vec<[u8; 4]> = values.iter().map(|v| v.to_be_bytes()).collect();
    interleave_bytes(&records)
}

/// Writes `values` as interleaved big-endian 64-bit words, the inverse of
/// [`Reader::interleaved_u64`].
pub(crate) fn interleave_u64(values: &[u64]) -> Vec<u8> {
    let records: Vec<[u8; 8]> = values.iter().map(|v| v.to_be_bytes()).collect();
    interleave_bytes(&records)
}

/// Writes `values` as interleaved, bit-rotated floats, the inverse of
/// [`Reader::interleaved_f32`].
pub(crate) fn interleave_f32(values: &[f32]) -> Vec<u8> {
    let raws: Vec<u32> = values.iter().map(|&v| rotated_f32_bits(v)).collect();
    interleave_u32(&raws)
}

/// Writes `values` zigzag-encoded (no delta) in interleaved layout.
///
/// Used by properties that zigzag but never delta-encode, unlike referent
/// arrays: Int32 and the offset half of UDim/UDim2.
pub(crate) fn interleave_zigzag_i32(values: &[i32]) -> Vec<u8> {
    let raws: Vec<u32> = values.iter().map(|&v| zigzag_encode_i32(v)).collect();
    interleave_u32(&raws)
}

/// 64-bit counterpart of [`interleave_zigzag_i32`], used by Int64 and SecurityCapabilities.
pub(crate) fn interleave_zigzag_i64(values: &[i64]) -> Vec<u8> {
    let raws: Vec<u64> = values.iter().map(|&v| zigzag_encode_i64(v)).collect();
    interleave_u64(&raws)
}

/// Writes a referent array delta- then zigzag-encoded, the inverse of
/// [`Reader::referents`]. Shared by INST, PRNT and Ref/Content property arrays.
pub(crate) fn encode_referents(values: &[i32]) -> Vec<u8> {
    let mut prev = 0i32;
    let raws: Vec<u32> = values
        .iter()
        .map(|&v| {
            let delta = zigzag_encode_i32(v.wrapping_sub(prev));
            prev = v;
            delta
        })
        .collect();
    interleave_u32(&raws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_round_trips_known_values() {
        assert_eq!(zigzag_i32(0), 0);
        assert_eq!(zigzag_i32(1), -1);
        assert_eq!(zigzag_i32(2), 1);
        assert_eq!(zigzag_i64(1), -1);
        // Sentinel used by Roblox for "no asset" / "no parent".
        assert_eq!(zigzag_i32(1), -1);
    }

    #[test]
    fn rotated_f32_decodes_one_and_zero() {
        // 1.0f32 is 0x3F800000; encoded it becomes a left rotation by one.
        assert_eq!(rotated_f32(0x3F80_0000u32.rotate_left(1)), 1.0);
        assert_eq!(rotated_f32(0), 0.0);
    }

    #[test]
    fn interleaved_u32_transposes_columns() {
        // Two elements 0x01020304 and 0x05060708 stored column by column.
        let payload = [0x01, 0x05, 0x02, 0x06, 0x03, 0x07, 0x04, 0x08];
        let mut reader = Reader::new(&payload);
        assert_eq!(
            reader.interleaved_u32(2).unwrap(),
            vec![0x0102_0304, 0x0506_0708]
        );
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn take_past_end_errors_instead_of_panicking() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert!(matches!(
            reader.take(4),
            Err(BinaryError::TooShort {
                expected: 4,
                actual: 3
            })
        ));
    }

    #[test]
    fn negative_length_is_rejected() {
        let payload = (-5i32).to_le_bytes();
        let mut reader = Reader::new(&payload);
        assert!(matches!(
            reader.length(),
            Err(BinaryError::InvalidLength(-5))
        ));
    }

    #[test]
    fn zigzag_encode_round_trips_decode() {
        for value in [0i32, -1, 1, i32::MIN, i32::MAX] {
            assert_eq!(zigzag_i32(zigzag_encode_i32(value)), value);
        }
        for value in [0i64, -1, 1, i64::MIN, i64::MAX] {
            assert_eq!(zigzag_i64(zigzag_encode_i64(value)), value);
        }
    }

    #[test]
    fn rotated_bits_round_trip_decode() {
        for value in [0.0f32, 1.0, -1.0, f32::MIN, f32::MAX] {
            assert_eq!(
                rotated_f32(rotated_f32_bits(value)).to_bits(),
                value.to_bits()
            );
        }
        for value in [0i64, -1, 1, i64::MIN, i64::MAX] {
            assert_eq!(rotated_i64(rotated_i64_bits(value)), value);
        }
    }

    #[test]
    fn interleave_u32_round_trips_the_reader() {
        let values = vec![0x0102_0304u32, 0x0506_0708];
        let payload = interleave_u32(&values);
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.interleaved_u32(values.len()).unwrap(), values);
    }

    #[test]
    fn interleave_bytes_round_trips_a_wide_record() {
        let records: Vec<[u8; 16]> = vec![
            std::array::from_fn(|i| i as u8),
            std::array::from_fn(|i| i as u8 + 0x10),
        ];
        let payload = interleave_bytes(&records);
        let mut reader = Reader::new(&payload);
        assert_eq!(
            reader.interleaved_bytes::<16>(records.len()).unwrap(),
            records
        );
    }

    #[test]
    fn encode_referents_round_trips_the_reader() {
        let values = vec![5, 6, 2, -1];
        let payload = encode_referents(&values);
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.referents(values.len()).unwrap(), values);
    }

    #[test]
    fn interleave_zigzag_round_trips_the_reader() {
        let values = vec![-3i32, 0, 42, i32::MIN];
        let payload = interleave_zigzag_i32(&values);
        let mut reader = Reader::new(&payload);
        let decoded: Vec<i32> = reader
            .interleaved_u32(values.len())
            .unwrap()
            .into_iter()
            .map(zigzag_i32)
            .collect();
        assert_eq!(decoded, values);
    }
}
