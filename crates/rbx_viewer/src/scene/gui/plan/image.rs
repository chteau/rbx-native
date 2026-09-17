//! Reads the image an `ImageLabel`/`ImageButton` shows, told apart from the
//! rest of a `GuiObject` by the property rather than the class name so an
//! `ImageButton` lands here exactly like an `ImageLabel` does.
//!
//! `ImageButton.HoverImage`/`PressedImage` (and their `*Content` twins) are
//! read nowhere: Roblox only shows them while the mouse is over or holding
//! down the button, states a single static frame has no way to be in.
//!
//! Everything here stays in the source image's own pixels or in an
//! unresolved `UDim2` — the box it eventually lands in, and the source
//! image's pixel *size*, are both unknown until the renderer has an
//! uploaded texture to ask (see `renderer::gui::atlas::Slot`).

use std::collections::BTreeMap;

use rbx_assets::AssetRef;
use rbx_dom::Variant;

use super::props::{alpha, color, enum_of, float, vector2};
use super::Span;
use crate::textures::asset_uri;

/// An axis-aligned pixel rectangle inside a source image, e.g. `SliceCenter`.
/// Kept distinct from `super::super::layout::Rect`'s screen-space box so the
/// two are never confused for one another.
///
/// `pub(crate)` rather than `pub(super)` like the rest of this module: unlike
/// `Fill`, this shape survives all the way to `renderer::gui::quads::image`,
/// which builds the nine slice quads from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelRect {
    pub(crate) min: [f32; 2],
    pub(crate) max: [f32; 2],
}

/// `Enum.ScaleType`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) enum ScaleMode {
    Stretch,
    /// `TileSize`, a `UDim2` resolved against the element's own box once one
    /// exists — that resolution is `super::super::layout::painted`'s job.
    Tile {
        size: Span,
    },
    /// `SliceCenter`/`SliceScale`. `center` is `None` where `SliceCenter` is
    /// unset: the docs never state what Roblox itself falls back to there,
    /// so an absent boundary is treated as the whole image (every border
    /// zero pixels wide, i.e. the same as `Stretch`) rather than guessed at.
    Slice {
        center: Option<PixelRect>,
        scale: f32,
    },
    /// Letterboxed inside the box, keeping the image's own aspect ratio.
    Fit,
    /// Filled and centred, keeping aspect ratio, overflow cut off.
    Crop,
}

/// `Enum.ScaleType`'s ordinals, straight off `enums/ScaleType.yaml`.
const STRETCH: u32 = 0;
const SLICE: u32 = 1;
const TILE: u32 = 2;
const FIT: u32 = 3;
const CROP: u32 = 4;

/// `Enum.ResamplerMode.Pixelated`; `Default` (bilinear) is 0.
const PIXELATED: u32 = 1;

#[derive(Clone)]
pub(in crate::scene::gui) struct Fill {
    pub(in crate::scene::gui) asset: AssetRef,
    pub(in crate::scene::gui) tint: [f32; 3],
    pub(in crate::scene::gui) alpha: f32,
    pub(in crate::scene::gui) scale: ScaleMode,
    /// `ImageRectOffset`/`ImageRectSize`, in the source image's own pixels.
    /// Per `ImageRectSize`'s own docs, a `rect_size` with either dimension
    /// `0` means "the entire image" rather than a degenerate sub-rect.
    pub(in crate::scene::gui) rect_offset: [f32; 2],
    pub(in crate::scene::gui) rect_size: [f32; 2],
    pub(in crate::scene::gui) pixelated: bool,
}

pub(in crate::scene::gui) fn fill(properties: &BTreeMap<String, Variant>) -> Option<Fill> {
    // `ImageContent` is the `Content`-typed property Studio now saves the
    // picture under; `Image` is the `ContentId` spelling every older place
    // carries, and a file written by a recent Studio holds both with only one
    // of them filled in.
    let uri = ["Image", "ImageContent"]
        .iter()
        .filter_map(|name| asset_uri(properties.get(*name)?))
        .find(|uri| !uri.is_empty())?;
    let asset = AssetRef::parse(uri).ok()?;
    if asset == AssetRef::Empty {
        return None;
    }

    Some(Fill {
        asset,
        tint: color(properties, "ImageColor3", [1.0, 1.0, 1.0]),
        alpha: alpha(properties, "ImageTransparency"),
        scale: scale_mode(properties),
        rect_offset: vector2(properties, "ImageRectOffset"),
        rect_size: vector2(properties, "ImageRectSize"),
        pixelated: enum_of(properties, "ResampleMode", 0) == PIXELATED,
    })
}

fn scale_mode(properties: &BTreeMap<String, Variant>) -> ScaleMode {
    match enum_of(properties, "ScaleType", STRETCH) {
        TILE => ScaleMode::Tile {
            size: super::span(properties, "TileSize"),
        },
        SLICE => ScaleMode::Slice {
            center: slice_center(properties),
            // "Defaults to 1.0" — the one default `SliceScale`'s own docs do
            // state.
            scale: float(properties, "SliceScale", 1.0),
        },
        FIT => ScaleMode::Fit,
        CROP => ScaleMode::Crop,
        _ => ScaleMode::Stretch,
    }
}

fn slice_center(properties: &BTreeMap<String, Variant>) -> Option<PixelRect> {
    match properties.get("SliceCenter") {
        Some(Variant::Rect(value)) => Some(PixelRect {
            min: [value.min.x, value.min.y],
            max: [value.max.x, value.max.y],
        }),
        _ => None,
    }
}
