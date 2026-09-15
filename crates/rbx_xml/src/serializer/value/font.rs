//! Encoder for Font. Mirrors `value::font`.

use rbx_dom::{Font, FontStyle};

use crate::serializer::writer::Writer;

pub(crate) fn font(writer: &mut Writer, name: &str, value: &Font) {
    writer.open("Font", &[("name", name)]);

    writer.open("Family", &[]);
    writer.leaf("url", &[], &value.family);
    writer.close("Family");

    writer.leaf("Weight", &[], &value.weight.to_string());
    writer.leaf("Style", &[], style_name(value.style));

    if let Some(cached) = &value.cached_face_id {
        writer.open("CachedFaceId", &[]);
        writer.leaf("url", &[], cached);
        writer.close("CachedFaceId");
    }

    writer.close("Font");
}

// `Other` has no documented XML spelling: xml.md's `Style` only lists Normal and
// Italic. Any text other than those two decodes to `Other(0xFF)` (see the
// reader's `style_from_name`), so an `Other` ordinal other than 0xFF cannot be
// represented exactly by this format regardless of what is written here.
fn style_name(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Italic => "Italic",
        FontStyle::Normal => "Normal",
        FontStyle::Other(_) => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_wraps_family_in_a_url_child() {
        let mut writer = Writer::new();
        font(
            &mut writer,
            "FontFace",
            &Font {
                family: "rbxasset://fonts/families/Arial.json".to_owned(),
                weight: 700,
                style: FontStyle::Italic,
                cached_face_id: None,
            },
        );
        let out = writer.into_string();
        assert!(out.contains("<url>rbxasset://fonts/families/Arial.json</url>"));
        assert!(out.contains("<Style>Italic</Style>"));
        assert!(!out.contains("CachedFaceId"));
    }
}
