//! Bounds-checked sequential reader over a mesh body.

use crate::error::MeshError;

// Mirrors `rbx_binary::codec::Reader`: every method is bounds-checked so a
// truncated or hostile file yields an error instead of a panic. Kept separate
// from that crate because the two error enums are deliberately independent.
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

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], MeshError> {
        if self.remaining() < len {
            return Err(MeshError::TooShort {
                expected: len,
                actual: self.remaining(),
            });
        }
        let out = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(out)
    }

    /// Takes `count` records of `stride` bytes as one slice.
    ///
    /// Taking the whole block up front is what keeps a hostile count from
    /// driving an allocation: the bytes must already be present before any
    /// `Vec` is sized from `count`.
    pub(crate) fn take_records(
        &mut self,
        count: usize,
        stride: usize,
    ) -> Result<&'a [u8], MeshError> {
        let len = count.checked_mul(stride).ok_or(MeshError::TooLarge)?;
        self.take(len)
    }

    /// Fails if any byte is left unread.
    ///
    /// Every block in the binary layouts is sized by the header, so a file that
    /// balances exactly is strong evidence the field widths are right. Checking it
    /// is the cheapest way to catch an off-by-a-few stride: a wrong width otherwise
    /// yields output that still looks plausible.
    pub(crate) fn expect_eof(&self) -> Result<(), MeshError> {
        match self.remaining() {
            0 => Ok(()),
            leftover => Err(MeshError::TrailingBytes(leftover)),
        }
    }

    pub(crate) fn u8(&mut self) -> Result<u8, MeshError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, MeshError> {
        // Slice length is guaranteed by take(), so try_into cannot fail.
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, MeshError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// Reads `count` little-endian `u32`s.
    pub(crate) fn u32_array(&mut self, count: usize) -> Result<Vec<u32>, MeshError> {
        let payload = self.take_records(count, 4)?;
        Ok(payload
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_past_end_errors_instead_of_panicking() {
        let mut reader = Reader::new(&[1, 2, 3]);
        assert!(matches!(
            reader.take(4),
            Err(MeshError::TooShort {
                expected: 4,
                actual: 3
            })
        ));
    }

    #[test]
    fn take_records_rejects_a_count_that_overflows() {
        let mut reader = Reader::new(&[0; 8]);
        assert!(matches!(
            reader.take_records(usize::MAX, 40),
            Err(MeshError::TooLarge)
        ));
    }

    #[test]
    fn take_records_rejects_a_count_the_input_cannot_back() {
        let mut reader = Reader::new(&[0; 8]);
        assert!(matches!(
            reader.take_records(1_000_000, 40),
            Err(MeshError::TooShort { .. })
        ));
    }

    #[test]
    fn u32_array_reads_little_endian_words() {
        let payload = [0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05];
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.u32_array(2).unwrap(), vec![0x0102_0304, 0x0506_0708]);
        assert_eq!(reader.remaining(), 0);
    }
}
