//! An `ImageLabel`/`ImageButton`'s image as far as a pixel-space [`Rect`]
//! can resolve it: the `UDim2` tile size becomes a repeat count here, while
//! `Fit`/`Crop` and a nine-slice wait for the renderer, which alone knows the
//! source image's pixel size.

use rbx_assets::AssetRef;

use super::super::plan::{Fill, ScaleMode};
use super::{PixelRect, Rect};

/// An `ImageLabel`/`ImageButton`'s `ScaleType`, resolved as far as this layer
/// can: a `Tile`'s `UDim2` is already turned into a repeat count (in
/// [`Painted::repeat`]), but `Fit`/`Crop`'s letterbox and crop math need the
/// source image's own pixel size, which only the renderer's atlas knows — see
/// `renderer::gui::quads::image`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ImageScale {
    Stretch,
    Tile,
    Slice {
        center: Option<PixelRect>,
        scale: f32,
    },
    Fit,
    Crop,
}

/// An `ImageLabel`/`ImageButton`'s image, resolved as far as a pixel-space
/// `Rect` allows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Painted {
    pub(crate) asset: AssetRef,
    pub(crate) tint: [f32; 3],
    pub(crate) alpha: f32,
    /// How many times the image repeats across the box under `Stretch`/
    /// `Tile`, 1 being a stretch; meaningless for the other scale types.
    pub(crate) repeat: [f32; 2],
    pub(crate) scale: ImageScale,
    /// `ImageRectOffset`/`ImageRectSize`, still in the source image's pixels.
    pub(crate) rect_offset: [f32; 2],
    pub(crate) rect_size: [f32; 2],
    pub(crate) pixelated: bool,
}

pub(super) fn painted(fill: &Fill, rect: &Rect) -> Painted {
    let (scale, repeat) = match fill.scale {
        ScaleMode::Stretch => (ImageScale::Stretch, [1.0, 1.0]),
        // A tile bigger than the box repeats less than once, which is
        // Roblox's own behaviour: `TileSize` is a size, not a count.
        ScaleMode::Tile { size } => {
            let tile = size.against(rect.size());
            let repeat = [ratio(rect.width, tile[0]), ratio(rect.height, tile[1])];
            (ImageScale::Tile, repeat)
        }
        ScaleMode::Slice { center, scale } => (ImageScale::Slice { center, scale }, [1.0, 1.0]),
        ScaleMode::Fit => (ImageScale::Fit, [1.0, 1.0]),
        ScaleMode::Crop => (ImageScale::Crop, [1.0, 1.0]),
    };
    Painted {
        asset: fill.asset.clone(),
        tint: fill.tint,
        alpha: fill.alpha,
        repeat,
        scale,
        rect_offset: fill.rect_offset,
        rect_size: fill.rect_size,
        pixelated: fill.pixelated,
    }
}

/// How many tiles of `tile` pixels fit across `extent`. A non-positive tile
/// would repeat infinitely often, so it falls back to a single stretch.
fn ratio(extent: f32, tile: f32) -> f32 {
    if tile > 0.0 {
        extent / tile
    } else {
        1.0
    }
}
