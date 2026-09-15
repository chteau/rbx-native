//! Encoder for UniqueId. Mirrors `value::identity`; inverts its exact byte
//! layout and `random`'s bit-rotation so the two agree on both directions.

use rbx_dom::UniqueId;

use crate::serializer::writer::Writer;

pub(crate) fn unique_id(writer: &mut Writer, name: &str, value: &UniqueId) {
    // Inverse of the reader's `random_raw.rotate_right(1) as i64`.
    let random_raw = (value.random as u64).rotate_left(1);

    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&random_raw.to_be_bytes());
    bytes[8..12].copy_from_slice(&value.time.to_be_bytes());
    bytes[12..16].copy_from_slice(&value.index.to_be_bytes());

    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    writer.leaf("UniqueId", &[("name", name)], &hex);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_reader_fixture_bytes() {
        let mut writer = Writer::new();
        unique_id(
            &mut writer,
            "Id",
            &UniqueId {
                index: 2,
                time: 0x09e5_50b2,
                random: 0x0038_2dbe_99a6_3c35,
            },
        );
        assert_eq!(
            writer.into_string(),
            "<UniqueId name=\"Id\">00705b7d334c786a09e550b200000002</UniqueId>\n"
        );
    }
}
