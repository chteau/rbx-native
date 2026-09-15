//! Sequential little-endian byte writer, the encode counterpart of `codec::Reader`.

pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i16(&mut self, value: i16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f64(&mut self, value: f64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.buf.extend_from_slice(value);
    }

    // Counts and string lengths are always written as i32, matching `Reader::length`.
    pub(crate) fn length(&mut self, len: usize) {
        self.buf.extend_from_slice(&(len as i32).to_le_bytes());
    }

    pub(crate) fn sized_bytes(&mut self, value: &[u8]) {
        self.length(value.len());
        self.bytes(value);
    }

    pub(crate) fn sized_name(&mut self, value: &str) {
        self.sized_bytes(value.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Reader;

    #[test]
    fn sized_name_round_trips_through_the_reader() {
        let mut writer = Writer::new();
        writer.sized_name("Workspace");
        let bytes = writer.into_bytes();

        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.sized_name().unwrap(), "Workspace");
    }

    #[test]
    fn scalar_fields_round_trip_through_the_reader() {
        let mut writer = Writer::new();
        writer.u8(0xAB);
        writer.u16(0x1234);
        writer.i16(-7);
        writer.f32(1.5);
        writer.f64(2.5);
        let bytes = writer.into_bytes();

        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.u8().unwrap(), 0xAB);
        assert_eq!(reader.u16().unwrap(), 0x1234);
        assert_eq!(reader.i16().unwrap(), -7);
        assert_eq!(reader.f32().unwrap(), 1.5);
        assert_eq!(reader.f64().unwrap(), 2.5);
    }
}
