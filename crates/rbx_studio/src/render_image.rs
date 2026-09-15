//! Shared conversion from decoded RGBA8 pixels into an image GPUI can paint.
//!
//! Both the 3D viewport's readback frames and the Explorer's sliced class
//! icons go through this: GPUI's texture atlas expects BGRA, so the swap only
//! needs writing (and testing) once.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};

/// Wraps tightly-packed, top-row-first RGBA8 pixels as a [`RenderImage`].
///
/// Returns `None` if `pixels` is too short for `width * height` pixels.
pub(crate) fn to_render_image(
    mut pixels: Vec<u8>,
    width: u32,
    height: u32,
) -> Option<Arc<RenderImage>> {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let buffer = RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::to_render_image;

    #[test]
    fn swaps_red_and_blue() {
        let pixels = vec![1, 2, 3, 255];
        let image = to_render_image(pixels, 1, 1).expect("a one pixel image");
        assert_eq!(image.as_bytes(0), Some([3, 2, 1, 255].as_slice()));
    }

    #[test]
    fn rejects_a_short_buffer() {
        assert!(to_render_image(vec![0; 4], 2, 2).is_none());
    }
}
