//! How large a face draws at a given `TextSize`.
//!
//! `TextSize` is a line box, not an em (Roblox's docs on `Font`: each font
//! renders "with the line height equal to the `TextSize` property"), and the
//! box is the face's own ascent plus descent, its `hhea` ascender less its
//! descender. Measured in Studio with `TextService:GetTextSize("Click me",
//! 100, ...)` against each face's advances, the em comes out at `100 / hhea`
//! within 1% for Arimo (1.117), Source Sans Pro (1.257), Roboto, Montserrat,
//! Fredoka One, Builder Sans, Nunito and Merriweather alike; the 1% left
//! over is Roblox rounding each advance to a whole pixel.

use std::collections::HashMap;

use rbx_assets::AssetRef;
use swash::{tag_from_bytes, FontRef};

use crate::fonts::{Face, Family};

/// The line box in ems for a face whose tables were never read: the system
/// fallback before a Roblox face lands. Source Sans Pro, the old default,
/// rounds its 1.257 to about this at UI sizes.
pub(super) const FALLBACK: f32 = 1.2;

/// `LegacyArial.json`, the face `Enum.Font.Legacy` maps to.
const LEGACY_FAMILY: &str = "fonts/families/LegacyArial.json";

/// How much taller than `TextSize` a `Legacy` line is. Studio's
/// `GetTextSize` gives `Legacy` at every size from 8 to 64 exactly what it
/// gives Arimo, the same face, at 1.5 times the size: "Click me" at 24 is
/// 127x36 in `Legacy` and in Arimo at 36.
const LEGACY_SCALE: f32 = 1.5;

/// `(ascender - descender) / unitsPerEm` off the face's `hhea` and `head`
/// tables, `None` for bytes that are not a font.
pub(super) fn line_ems(bytes: &[u8]) -> Option<f32> {
    let font = FontRef::from_index(bytes, 0)?;
    let int16 =
        |table: &[u8], at: usize| Some(i16::from_be_bytes([*table.get(at)?, *table.get(at + 1)?]));
    let hhea = font.table(tag_from_bytes(b"hhea"))?;
    let head = font.table(tag_from_bytes(b"head"))?;
    let units = u16::from_be_bytes([*head.get(18)?, *head.get(19)?]);
    let extent = f32::from(int16(hhea, 4)?) - f32::from(int16(hhea, 6)?);
    (units > 0 && extent > 0.0).then(|| extent / f32::from(units))
}

/// The line box `face` draws in, in ems: the face that landed nearest the
/// request, as [`super::attrs`] picks it.
pub(super) fn line_em(
    families: &HashMap<AssetRef, Family>,
    ems: &HashMap<AssetRef, f32>,
    face: &Face,
) -> f32 {
    families
        .get(&face.family)
        .and_then(|family| family.closest(face.weight, face.italic))
        .and_then(|entry| ems.get(&entry.asset))
        .copied()
        .unwrap_or(FALLBACK)
}

/// The line box `face` takes at `size`, which only `Legacy` makes larger.
pub(super) fn line_box(face: &Face, size: f32) -> f32 {
    match face.family == AssetRef::Native(LEGACY_FAMILY.to_owned()) {
        true => size * LEGACY_SCALE,
        false => size,
    }
}
