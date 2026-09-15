//! Encoders for scalar leaf types: string, bool, int, int64, float, double,
//! token, BrickColor. Mirrors `value::scalar`, the reader's decoder counterpart.

use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::serializer::writer::Writer;

/// Always writes the canonical `string` tag: the reader decodes `ProtectedString`
/// identically (see `value::mod`'s dispatch), and `Variant::String` carries no bit
/// remembering which spelling produced it, so there is nothing to pick between.
pub(crate) fn string(writer: &mut Writer, name: &str, text: &str) {
    writer.leaf("string", &[("name", name)], text);
}

pub(crate) fn bool_value(writer: &mut Writer, name: &str, value: bool) {
    writer.leaf(
        "bool",
        &[("name", name)],
        if value { "true" } else { "false" },
    );
}

pub(crate) fn int32(writer: &mut Writer, name: &str, value: i32) {
    writer.leaf("int", &[("name", name)], &value.to_string());
}

pub(crate) fn int64(writer: &mut Writer, name: &str, value: i64) {
    writer.leaf("int64", &[("name", name)], &value.to_string());
}

// Rust's `f32`/`f64` `Display` already emits the shortest string that parses back
// to the exact same bit pattern, and (per the reader's own `parse_f32` comment)
// already spells INF/-INF/NaN the way `f32::from_str`/`f64::from_str` expect, so no
// custom formatting is needed for either finite or non-finite values.
pub(crate) fn float32(writer: &mut Writer, name: &str, value: f32) {
    writer.leaf("float", &[("name", name)], &value.to_string());
}

pub(crate) fn float64(writer: &mut Writer, name: &str, value: f64) {
    writer.leaf("double", &[("name", name)], &value.to_string());
}

pub(crate) fn enum_value(writer: &mut Writer, name: &str, value: u32) {
    writer.leaf("token", &[("name", name)], &value.to_string());
}

pub(crate) fn brick_color(writer: &mut Writer, name: &str, value: u32) {
    writer.leaf("BrickColor", &[("name", name)], &value.to_string());
}

/// Writes an opaque `Variant::Unknown` blob as `BinaryString`, the only XML type
/// that carries arbitrary bytes. The caller (`serializer::value::encode`) only
/// reaches this for a blob whose `type_id` is already `STRING_TYPE_ID`, since
/// `BinaryString` has no side channel for an arbitrary wire type id: any other
/// `type_id` would come back tagged `STRING_TYPE_ID` on read-back instead of its
/// original one, so it is rejected as `Unsupported` before it gets here.
pub(crate) fn binary_string(writer: &mut Writer, name: &str, raw: &[u8]) {
    writer.leaf("BinaryString", &[("name", name)], &STANDARD.encode(raw));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_round_trips_infinity_spelling() {
        let mut writer = Writer::new();
        float32(&mut writer, "X", f32::NEG_INFINITY);
        assert_eq!(writer.into_string(), "<float name=\"X\">-inf</float>\n");
    }

    #[test]
    fn string_uses_the_plain_tag() {
        let mut writer = Writer::new();
        string(&mut writer, "Name", "Baseplate");
        assert_eq!(
            writer.into_string(),
            "<string name=\"Name\">Baseplate</string>\n"
        );
    }
}
