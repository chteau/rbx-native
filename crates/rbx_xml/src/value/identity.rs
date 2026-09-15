//! Decoder for UniqueId: a 16-byte hex string, ordered differently than the
//! binary format's own UniqueId payload (see xml.md's note on field order and on
//! the extra bit-rotation the XML encoding applies to `Random`).

use rbx_dom::{UniqueId, Variant};

pub(crate) fn unique_id(text: &str) -> Variant {
    let zero = UniqueId {
        index: 0,
        time: 0,
        random: 0,
    };
    let Some(bytes) = decode_hex(text) else {
        return Variant::UniqueId(zero);
    };
    let [b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15]: [u8; 16] =
        match bytes.try_into() {
            Ok(bytes) => bytes,
            Err(_) => return Variant::UniqueId(zero),
        };

    let random_raw = u64::from_be_bytes([b0, b1, b2, b3, b4, b5, b6, b7]);
    let time = u32::from_be_bytes([b8, b9, b10, b11]);
    let index = u32::from_be_bytes([b12, b13, b14, b15]);

    Variant::UniqueId(UniqueId {
        index,
        time,
        // Rotated left by one bit on the wire, the same trick the binary format
        // uses to keep the sign bit out of a fixed interleaved column; undo it
        // the same way (`rotate_right`) so both formats agree on this field.
        random: random_raw.rotate_right(1) as i64,
    })
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_same_random_component_as_the_binary_fixture() {
        // Random (raw wire bytes, before the rotate), then Time, then Index -
        // the exact bytes rbx_binary's own UniqueId fixture test decodes, just
        // reordered per xml.md's field layout.
        let hex = "00705b7d334c786a09e550b200000002";
        assert_eq!(
            unique_id(hex),
            Variant::UniqueId(UniqueId {
                index: 2,
                time: 0x09e5_50b2,
                random: 0x0038_2dbe_99a6_3c35
            })
        );
    }

    #[test]
    fn malformed_hex_degrades_to_zero() {
        assert_eq!(
            unique_id("not-hex"),
            Variant::UniqueId(UniqueId {
                index: 0,
                time: 0,
                random: 0
            })
        );
    }
}
