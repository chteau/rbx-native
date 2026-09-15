//! Decoders for scalar leaf types: string, BinaryString, bool, int, int64,
//! float, double, token, BrickColor.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rbx_dom::Variant;

use super::STRING_TYPE_ID;
use crate::xml_tree::Node;

/// `string`/`ProtectedString` are always valid UTF-8 here, since the XML tree
/// parser already produced a `String`; the non-UTF-8 branch only matters for the
/// other callers that hand this raw bytes decoded from Base64 or a shared-string
/// table entry.
pub(crate) fn string_value(bytes: &[u8]) -> Variant {
    match std::str::from_utf8(bytes) {
        Ok(text) => Variant::String(text.to_owned()),
        Err(_) => Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw: bytes.to_vec(),
        },
    }
}

pub(crate) fn binary_string(node: &Node) -> Variant {
    match STANDARD.decode(node.text_trim()) {
        Ok(bytes) => string_value(&bytes),
        // A malformed Base64 payload keeps the still-encoded text instead of
        // losing the property entirely.
        Err(_) => Variant::Unknown {
            type_id: STRING_TYPE_ID,
            raw: node.text_trim().as_bytes().to_vec(),
        },
    }
}

pub(crate) fn bool_value(text: &str) -> Variant {
    // xml.md: Roblox accepts case variations, even though it only ever writes
    // lowercase itself.
    Variant::Bool(text.eq_ignore_ascii_case("true"))
}

pub(crate) fn int32(text: &str) -> Variant {
    Variant::Int32(text.parse().unwrap_or(0))
}

pub(crate) fn int64(text: &str) -> Variant {
    Variant::Int64(text.parse().unwrap_or(0))
}

pub(crate) fn float32(text: &str) -> Variant {
    Variant::Float32(parse_f32(text))
}

pub(crate) fn float64(text: &str) -> Variant {
    Variant::Float64(text.parse().unwrap_or(0.0))
}

pub(crate) fn enum_value(text: &str) -> Variant {
    Variant::Enum(text.parse().unwrap_or(0))
}

pub(crate) fn brick_color(text: &str) -> Variant {
    Variant::BrickColor(text.parse().unwrap_or(0))
}

/// Parses a `float`/`double` textual value. Rust's own float parser already
/// accepts the `INF`/`+INF`/`-INF`/`NAN` spellings xml.md requires (it matches
/// them case-insensitively), so no extra handling is needed beyond a graceful
/// fallback for genuinely malformed text.
pub(crate) fn parse_f32(text: &str) -> f32 {
    text.trim().parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_accepts_any_case() {
        assert_eq!(bool_value("TrUe"), Variant::Bool(true));
        assert_eq!(bool_value("false"), Variant::Bool(false));
    }

    #[test]
    fn float_accepts_xsd_infinity_and_nan_spellings() {
        assert_eq!(float32("INF"), Variant::Float32(f32::INFINITY));
        assert_eq!(float32("-INF"), Variant::Float32(f32::NEG_INFINITY));
        assert!(matches!(float32("NAN"), Variant::Float32(v) if v.is_nan()));
    }

    #[test]
    fn malformed_int_degrades_to_zero() {
        assert_eq!(int32("not a number"), Variant::Int32(0));
    }

    #[test]
    fn binary_string_decodes_base64() {
        // Base64 for "Rojo is cool!", the exact worked example from xml.md.
        let node = Node {
            tag: "BinaryString".into(),
            text: "Um9qbyBpcyBjb29sIQ==".into(),
            ..Node::default()
        };
        assert_eq!(
            binary_string(&node),
            Variant::String("Rojo is cool!".to_owned())
        );
    }
}
