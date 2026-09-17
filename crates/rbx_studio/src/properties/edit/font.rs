//! The `FontFace` row and its legacy `Font` twin: how a face is typed as
//! text, read back, and kept in step with the enum that names it.

use rbx_dom::{Font, FontStyle, Variant};

/// The legacy `Font` enum and the `FontFace` it stands for: Roblox's docs on
/// `TextLabel.Font` say the two are "kept in sync", so a write to either
/// here writes the other too (see [`super::commit`]).
const FONT_PROPERTY: &str = "Font";
const FONT_FACE_PROPERTY: &str = "FontFace";

/// `Enum.FontWeight`'s members, from Roblox's rich text guide ("`Thin`,
/// `ExtraLight`, `Light`, `Regular`, `Medium`, `SemiBold`, `Bold`,
/// `ExtraBold`, or `Heavy`; ... a number in factors of 100 between `100` and
/// `900`") — so a weight edits by name where it has one.
const FONT_WEIGHTS: [(&str, u16); 9] = [
    ("Thin", 100),
    ("ExtraLight", 200),
    ("Light", 300),
    ("Regular", 400),
    ("Medium", 500),
    ("SemiBold", 600),
    ("Bold", 700),
    ("ExtraBold", 800),
    ("Heavy", 900),
];

/// Where Roblox's own families live; a bare family name in a `FontFace` edit
/// is completed to this, the way Studio's own picker names them.
const FAMILY_PREFIX: &str = "rbxasset://fonts/families/";
const FAMILY_SUFFIX: &str = ".json";

/// `Family, Weight, Style`: a Roblox family by its bare name (any other
/// source by its full URI), the weight by name where `Enum.FontWeight` has
/// one, the style as `Normal`/`Italic`.
pub(super) fn font_text(font: &Font) -> String {
    let family = font
        .family
        .strip_prefix(FAMILY_PREFIX)
        .and_then(|rest| rest.strip_suffix(FAMILY_SUFFIX))
        .unwrap_or(&font.family);
    let weight = FONT_WEIGHTS
        .iter()
        .find(|(_, value)| *value == font.weight)
        .map_or_else(|| font.weight.to_string(), |(name, _)| (*name).to_owned());
    let style = match font.style {
        FontStyle::Normal => "Normal".to_owned(),
        FontStyle::Italic => "Italic".to_owned(),
        FontStyle::Other(raw) => raw.to_string(),
    };
    format!("{family}, {weight}, {style}")
}

/// The reverse of [`font_text`]; names are case-insensitive and a weight may
/// be its number. The face id Studio caches alongside is dropped: it named
/// the old face.
pub(super) fn parse_font(text: &str) -> Result<Font, String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [family, weight, style] = parts[..] else {
        return Err(format!(
            "expected family, weight, style, got {} values",
            parts.len()
        ));
    };
    let family = match family.contains("://") {
        true => family.to_owned(),
        false => format!("{FAMILY_PREFIX}{family}{FAMILY_SUFFIX}"),
    };
    let weight = match weight.parse::<u16>() {
        Ok(number) => number,
        Err(_) => FONT_WEIGHTS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(weight))
            .map(|(_, value)| *value)
            .ok_or_else(|| format!("{weight:?} is not a FontWeight"))?,
    };
    let style = match style.parse::<u8>() {
        Ok(raw) => FontStyle::from(raw),
        Err(_) if style.eq_ignore_ascii_case("normal") => FontStyle::Normal,
        Err(_) if style.eq_ignore_ascii_case("italic") => FontStyle::Italic,
        Err(_) => return Err(format!("{style:?} is not Normal or Italic")),
    };
    Ok(Font {
        family,
        weight,
        style,
        cached_face_id: None,
    })
}

/// The property a `Font`/`FontFace` write keeps in sync, with the value it
/// takes; `None` for any other property, and for a `Font` the enum has no
/// face for (`Unknown`), which leaves `FontFace` as it was.
pub(super) fn synced(prop_name: &str, value: &Variant) -> Option<(&'static str, Variant)> {
    match (prop_name, value) {
        (FONT_PROPERTY, Variant::Enum(raw)) => {
            Some((FONT_FACE_PROPERTY, Variant::Font(Font::from_legacy(*raw)?)))
        }
        (FONT_FACE_PROPERTY, Variant::Font(font)) => {
            Some((FONT_PROPERTY, Variant::Enum(font.legacy())))
        }
        _ => None,
    }
}
