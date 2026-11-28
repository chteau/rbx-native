//! Identity and asset-reference property value types (UniqueId, Font, Content).

use crate::reference::Ref;

/// A per-instance identifier that Roblox Studio keeps stable across saves.
///
/// `time` counts seconds since 2021-01-01, not the Unix epoch. The binary and XML
/// formats disagree on both field order and on the encoding of `random`, so the
/// fields are kept separate rather than collapsed into one opaque 128-bit value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UniqueId {
    pub index: u32,
    pub time: u32,
    pub random: i64,
}

/// Slant of a font face, as the `Enum.FontStyle` ordinal it is stored with.
///
/// `Other` preserves an ordinal Roblox added after this parser was written instead
/// of silently collapsing it onto `Normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Other(u8),
}

impl From<u8> for FontStyle {
    fn from(raw: u8) -> Self {
        match raw {
            0 => FontStyle::Normal,
            1 => FontStyle::Italic,
            other => FontStyle::Other(other),
        }
    }
}

/// The ordinal `FontStyle` was read from, which is what the attribute and
/// binary formats both store, see `attributes::encode`.
impl From<FontStyle> for u8 {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            FontStyle::Other(raw) => raw,
        }
    }
}

/// A font face: a family asset plus the weight and slant selected inside it.
///
/// `weight` is the raw `Enum.FontWeight` value, which is the numeric weight itself
/// (400 for regular, 700 for bold), so it is kept as a number rather than an enum.
/// `cached_face_id` is a Studio-internal resolution cache and is often absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub family: String,
    pub weight: u16,
    pub style: FontStyle,
    pub cached_face_id: Option<String>,
}

/// `Enum.Font` → (family file name, weight, italic): the `FontFace` Studio
/// 0.748 gives a `TextLabel` whose `Font` is set to each member, read back in
/// its command bar. `Arial*` and `Gotham*` keep naming `Arial.json` and
/// `GothamSSm.json`, families the client no longer ships and draws in Arimo
/// and Montserrat (see `rbx_viewer`'s `REPLACED_FAMILIES`). `Unknown` (100)
/// has no row.
const LEGACY_FONTS: [(u32, &str, u16, bool); 52] = [
    (0, "LegacyArial", 400, false),
    (1, "Arial", 400, false),
    (2, "Arial", 700, false),
    (3, "SourceSansPro", 400, false),
    (4, "SourceSansPro", 700, false),
    (5, "SourceSansPro", 300, false),
    (6, "SourceSansPro", 400, true),
    (7, "AccanthisADFStd", 400, false),
    (8, "Guru", 400, false),
    (9, "ComicNeueAngular", 400, false),
    (10, "Inconsolata", 400, false),
    (11, "HighwayGothic", 400, false),
    (12, "Zekton", 400, false),
    (13, "PressStart2P", 400, false),
    (14, "Balthazar", 400, false),
    (15, "RomanAntique", 400, false),
    (16, "SourceSansPro", 600, false),
    (17, "GothamSSm", 400, false),
    (18, "GothamSSm", 500, false),
    (19, "GothamSSm", 700, false),
    (20, "GothamSSm", 900, false),
    (21, "AmaticSC", 400, false),
    (22, "Bangers", 400, false),
    (23, "Creepster", 400, false),
    (24, "DenkOne", 400, false),
    (25, "Fondamento", 400, false),
    (26, "FredokaOne", 400, false),
    (27, "GrenzeGotisch", 400, false),
    (28, "IndieFlower", 400, false),
    (29, "JosefinSans", 400, false),
    (30, "Jura", 400, false),
    (31, "Kalam", 400, false),
    (32, "LuckiestGuy", 400, false),
    (33, "Merriweather", 400, false),
    (34, "Michroma", 400, false),
    (35, "Nunito", 400, false),
    (36, "Oswald", 400, false),
    (37, "PatrickHand", 400, false),
    (38, "PermanentMarker", 400, false),
    (39, "Roboto", 400, false),
    (40, "RobotoCondensed", 400, false),
    (41, "RobotoMono", 400, false),
    (42, "Sarpanch", 400, false),
    (43, "SpecialElite", 400, false),
    (44, "TitilliumWeb", 400, false),
    (45, "Ubuntu", 400, false),
    (46, "BuilderSans", 400, false),
    (47, "BuilderSans", 500, false),
    (48, "BuilderSans", 700, false),
    (49, "BuilderSans", 800, false),
    (50, "Arimo", 400, false),
    (51, "Arimo", 700, false),
];

/// `Enum.Font.Unknown`: what `Font` reads once `FontFace` names a face no
/// enum member stands for.
pub const FONT_UNKNOWN: u32 = 100;

impl Font {
    /// The `FontFace` a legacy `Font` enum ordinal stands for, the docs on
    /// `TextLabel.Font` say the two properties are "kept in sync", or `None`
    /// for `Unknown` and any ordinal a newer client may add.
    pub fn from_legacy(font: u32) -> Option<Font> {
        let &(_, family, weight, italic) = LEGACY_FONTS.iter().find(|row| row.0 == font)?;
        Some(Font {
            family: format!("rbxasset://fonts/families/{family}.json"),
            weight,
            style: match italic {
                true => FontStyle::Italic,
                false => FontStyle::Normal,
            },
            cached_face_id: None,
        })
    }

    /// The legacy `Font` enum ordinal for this face, [`FONT_UNKNOWN`] when no
    /// member names it (a cloud family, a weight the enum never had).
    pub fn legacy(&self) -> u32 {
        LEGACY_FONTS
            .iter()
            .find(|&&(_, family, weight, italic)| {
                self.weight == weight
                    && (self.style == FontStyle::Italic) == italic
                    && self.family == format!("rbxasset://fonts/families/{family}.json")
            })
            .map_or(FONT_UNKNOWN, |row| row.0)
    }
}

/// Source of a `Content` property: nothing, an asset URI, or another instance.
///
/// Roblox also serializes a fourth list of *external* referents, used only while
/// copy-pasting between documents. Those referents point outside the file and can
/// never be resolved from one, so they have no variant here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Content {
    #[default]
    None,
    Uri(String),
    Object(Ref),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_legacy_font_round_trips_through_its_face() {
        let bold = Font::from_legacy(4).unwrap();
        assert_eq!(bold.family, "rbxasset://fonts/families/SourceSansPro.json");
        assert_eq!((bold.weight, bold.style), (700, FontStyle::Normal));
        assert_eq!(bold.legacy(), 4);
        assert_eq!(Font::from_legacy(6).unwrap().style, FontStyle::Italic);
        assert_eq!(Font::from_legacy(FONT_UNKNOWN), None);
        assert_eq!(Font::from_legacy(50).unwrap().legacy(), 50);

        let cloud = Font {
            family: "rbxassetid://12187365364".to_string(),
            weight: 400,
            style: FontStyle::Normal,
            cached_face_id: None,
        };
        assert_eq!(cloud.legacy(), FONT_UNKNOWN);
    }

    /// Every member Studio 0.748 lists, with the `FontFace` it gives a
    /// `TextLabel` set to it: `Enum.Font:GetEnumItems()` walked in the command
    /// bar, `label.Font = item`, then `label.FontFace` read back.
    const STUDIO: [(u32, &str, &str, u16, bool); 52] = [
        (0, "Legacy", "LegacyArial", 400, false),
        (1, "Arial", "Arial", 400, false),
        (2, "ArialBold", "Arial", 700, false),
        (3, "SourceSans", "SourceSansPro", 400, false),
        (4, "SourceSansBold", "SourceSansPro", 700, false),
        (5, "SourceSansLight", "SourceSansPro", 300, false),
        (6, "SourceSansItalic", "SourceSansPro", 400, true),
        (7, "Bodoni", "AccanthisADFStd", 400, false),
        (8, "Garamond", "Guru", 400, false),
        (9, "Cartoon", "ComicNeueAngular", 400, false),
        (10, "Code", "Inconsolata", 400, false),
        (11, "Highway", "HighwayGothic", 400, false),
        (12, "SciFi", "Zekton", 400, false),
        (13, "Arcade", "PressStart2P", 400, false),
        (14, "Fantasy", "Balthazar", 400, false),
        (15, "Antique", "RomanAntique", 400, false),
        (16, "SourceSansSemibold", "SourceSansPro", 600, false),
        (17, "Gotham", "GothamSSm", 400, false),
        (18, "GothamMedium", "GothamSSm", 500, false),
        (19, "GothamBold", "GothamSSm", 700, false),
        (20, "GothamBlack", "GothamSSm", 900, false),
        (21, "AmaticSC", "AmaticSC", 400, false),
        (22, "Bangers", "Bangers", 400, false),
        (23, "Creepster", "Creepster", 400, false),
        (24, "DenkOne", "DenkOne", 400, false),
        (25, "Fondamento", "Fondamento", 400, false),
        (26, "FredokaOne", "FredokaOne", 400, false),
        (27, "GrenzeGotisch", "GrenzeGotisch", 400, false),
        (28, "IndieFlower", "IndieFlower", 400, false),
        (29, "JosefinSans", "JosefinSans", 400, false),
        (30, "Jura", "Jura", 400, false),
        (31, "Kalam", "Kalam", 400, false),
        (32, "LuckiestGuy", "LuckiestGuy", 400, false),
        (33, "Merriweather", "Merriweather", 400, false),
        (34, "Michroma", "Michroma", 400, false),
        (35, "Nunito", "Nunito", 400, false),
        (36, "Oswald", "Oswald", 400, false),
        (37, "PatrickHand", "PatrickHand", 400, false),
        (38, "PermanentMarker", "PermanentMarker", 400, false),
        (39, "Roboto", "Roboto", 400, false),
        (40, "RobotoCondensed", "RobotoCondensed", 400, false),
        (41, "RobotoMono", "RobotoMono", 400, false),
        (42, "Sarpanch", "Sarpanch", 400, false),
        (43, "SpecialElite", "SpecialElite", 400, false),
        (44, "TitilliumWeb", "TitilliumWeb", 400, false),
        (45, "Ubuntu", "Ubuntu", 400, false),
        (46, "BuilderSans", "BuilderSans", 400, false),
        (47, "BuilderSansMedium", "BuilderSans", 500, false),
        (48, "BuilderSansBold", "BuilderSans", 700, false),
        (49, "BuilderSansExtraBold", "BuilderSans", 800, false),
        (50, "Arimo", "Arimo", 400, false),
        (51, "ArimoBold", "Arimo", 700, false),
    ];

    #[test]
    fn every_enum_font_names_the_face_studio_gives_it() {
        for (ordinal, name, family, weight, italic) in STUDIO {
            let font = Font::from_legacy(ordinal).unwrap_or_else(|| panic!("{name} has no face"));
            assert_eq!(
                (
                    font.family.as_str(),
                    font.weight,
                    font.style == FontStyle::Italic
                ),
                (
                    format!("rbxasset://fonts/families/{family}.json").as_str(),
                    weight,
                    italic
                ),
                "Enum.Font.{name}"
            );
            assert_eq!(font.legacy(), ordinal, "Enum.Font.{name} round-trips");
        }
    }
}
