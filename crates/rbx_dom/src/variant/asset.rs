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
