//! Decoder for the Font property type.

use rbx_dom::{Font, Variant};

use super::PropValues;
use crate::codec::Reader;
use crate::error::BinaryError;

/// Reads a Font property array.
///
/// Sequential per instance, with no interleaving: family URI, weight, style, then
/// the cached face URI. Both strings are length-prefixed, which rules out a column
/// layout; `Weight` and `Style` are little-endian even though they read as enums.
pub(super) fn fonts(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            // Both fields are content URIs, i.e. ASCII in practice; a lossy
            // conversion keeps a mangled file readable instead of failing it.
            let family = reader.sized_name()?;
            let weight = reader.u16()?;
            let style = reader.u8()?.into();
            let cached_face_id = reader.sized_name()?;

            Ok(Some(Variant::Font(Font {
                family,
                weight,
                style,
                // Studio writes a zero-length string when it has no resolved face
                // cached, which is absence rather than an empty URI.
                cached_face_id: (!cached_face_id.is_empty()).then_some(cached_face_id),
            })))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rbx_dom::FontStyle;

    use super::*;

    fn encoded(family: &str, weight: u16, style: u8, cached: &str) -> Vec<u8> {
        let mut payload = (family.len() as i32).to_le_bytes().to_vec();
        payload.extend_from_slice(family.as_bytes());
        payload.extend_from_slice(&weight.to_le_bytes());
        payload.push(style);
        payload.extend_from_slice(&(cached.len() as i32).to_le_bytes());
        payload.extend_from_slice(cached.as_bytes());
        payload
    }

    // Byte-for-byte the ChannelTabsConfiguration.FontFace payload of
    // TestPlace.rbxl, which is 90 bytes long: 4 + 42 + 2 + 1 + 4 + 37.
    #[test]
    fn font_reads_the_real_channel_tabs_face() {
        let payload = encoded(
            "rbxasset://fonts/families/BuilderSans.json",
            700,
            0,
            "rbxasset://fonts/BuilderSans-Bold.otf",
        );
        assert_eq!(payload.len(), 90);

        let values = fonts(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Font(Font {
                family: "rbxasset://fonts/families/BuilderSans.json".to_owned(),
                weight: 700,
                style: FontStyle::Normal,
                cached_face_id: Some("rbxasset://fonts/BuilderSans-Bold.otf".to_owned()),
            }))
        );
    }

    #[test]
    fn empty_cached_face_id_becomes_absent() {
        let payload = encoded("rbxasset://fonts/families/Arial.json", 400, 1, "");
        let values = fonts(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Font(Font {
                family: "rbxasset://fonts/families/Arial.json".to_owned(),
                weight: 400,
                style: FontStyle::Italic,
                cached_face_id: None,
            }))
        );
    }

    #[test]
    fn two_fonts_are_read_back_to_back() {
        let mut payload = encoded("a", 400, 0, "b");
        payload.extend(encoded("c", 200, 9, ""));

        let values = fonts(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(values.len(), 2);
        assert_eq!(
            values[1],
            Some(Variant::Font(Font {
                family: "c".to_owned(),
                weight: 200,
                // An ordinal Roblox has not defined yet is preserved, not defaulted.
                style: FontStyle::Other(9),
                cached_face_id: None,
            }))
        );
    }
}
