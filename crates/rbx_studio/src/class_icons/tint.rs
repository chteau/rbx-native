//! Recoloring an already-rasterized icon to a `Folder`'s own tag colour —
//! split out of `class_icons.rs` to keep that file (already bulky with its
//! 300-class slug table) under `GUIDELINES.md` §6's line budget.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// Recolors every non-transparent pixel of an already-rasterized icon to
/// `color` (0-255 sRGB, the space `properties::EditKind::Color` already
/// edits in), keeping each pixel's own alpha — so an anti-aliased edge stays
/// exactly as soft as the original, just a flat tag colour instead of the
/// tile's own palette. A byte-buffer pass over the existing bitmap rather
/// than another `resvg` render: the shape (and, cheaply, the file's own
/// rounded corners and anti-aliasing) comes along for free, and the caller
/// (`explorer::items`) already caches the result per tag colour so this only
/// runs once per colour actually in use, not once per tagged instance.
///
/// `RenderImage` stores BGRA bytes (see its own doc comment and
/// `render_image::to_render_image`, which is what produced `image` in the
/// first place) — this writes that same layout back out, so the result
/// needs no further channel swap.
pub(crate) fn tint(image: &RenderImage, color: (u8, u8, u8)) -> Option<Arc<RenderImage>> {
    let bytes = image.as_bytes(0)?;
    let size = image.size(0);
    let (width, height): (u32, u32) = (size.width.into(), size.height.into());
    let (r, g, b) = color;

    let tinted: Vec<u8> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [b, g, r, pixel[3]])
        .collect();

    let buffer = RgbaImage::from_raw(width, height, tinted)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::tint;
    use crate::class_icons::icon_tile;

    /// The folder tile's own two shades (see `assets/icons/default/dark/
    /// folder.svg`) both become the tag colour; fully transparent pixels
    /// (the tile is smaller than its 32x32 canvas) stay transparent, and
    /// every pixel's own alpha survives unchanged.
    #[test]
    fn tint_flattens_every_opaque_pixel_to_the_tag_colour_and_keeps_alpha() {
        let folder = icon_tile("Folder").expect("the folder tile rasterizes");
        let tinted = tint(&folder, (10, 20, 30)).expect("tinting a real tile never fails");

        let before = folder.as_bytes(0).unwrap();
        let after = tinted.as_bytes(0).unwrap();
        assert_eq!(before.len(), after.len());

        for (original, tinted) in before
            .as_chunks::<4>()
            .0
            .iter()
            .zip(after.as_chunks::<4>().0)
        {
            // BGRA — see this module's `tint` doc comment.
            assert_eq!(original[3], tinted[3], "alpha must survive untouched");
            if tinted[3] == 0 {
                continue;
            }
            assert_eq!(tinted, &[30, 20, 10, tinted[3]]);
        }
        // The tile does have some opaque pixels — otherwise the assertions
        // above would vacuously pass without ever exercising the tint.
        assert!(after.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn tint_preserves_the_source_images_dimensions() {
        let folder = icon_tile("Folder").expect("the folder tile rasterizes");
        let tinted = tint(&folder, (255, 0, 0)).expect("tinting a real tile never fails");

        assert_eq!(folder.size(0).width, tinted.size(0).width);
        assert_eq!(folder.size(0).height, tinted.size(0).height);
    }
}
