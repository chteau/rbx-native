//! Font encoding, the encode counterpart of `chunks::prop::font`.

use rbx_dom::{FontStyle, Variant};

use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

pub(super) fn fonts(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let fonts = map_dense(class, name, values, |v| match v {
        Variant::Font(f) => Some(f.clone()),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for font in fonts {
        writer.sized_name(&font.family);
        writer.u16(font.weight);
        writer.u8(style_byte(font.style));
        // Absence is written as a zero-length string, matching what Studio does when
        // it has no resolved face cached.
        writer.sized_name(font.cached_face_id.as_deref().unwrap_or(""));
    }
    Ok(writer.into_bytes())
}

fn style_byte(style: FontStyle) -> u8 {
    match style {
        FontStyle::Normal => 0,
        FontStyle::Italic => 1,
        FontStyle::Other(raw) => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::Font;

    fn decoded(type_id: u8, count: usize, payload: &[u8]) -> Vec<Option<Variant>> {
        let header = PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        };
        decode(&header, count, &[])
    }

    #[test]
    fn font_round_trips_with_and_without_a_cached_face() {
        let values = vec![
            Some(Variant::Font(Font {
                family: "rbxasset://fonts/families/BuilderSans.json".to_owned(),
                weight: 700,
                style: FontStyle::Normal,
                cached_face_id: Some("rbxasset://fonts/BuilderSans-Bold.otf".to_owned()),
            })),
            Some(Variant::Font(Font {
                family: "rbxasset://fonts/families/Arial.json".to_owned(),
                weight: 400,
                style: FontStyle::Other(9),
                cached_face_id: None,
            })),
        ];
        let payload = fonts("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x20, 2, &payload), values);
    }
}
