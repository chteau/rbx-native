//! Where the 3D view sits in its window, and how a rendered frame becomes an
//! image GPUI can paint.

use std::sync::Arc;

use gpui_kit::{Pixels, RenderImage};

use crate::render_image::to_render_image;

/// The viewport's place in the window, in physical pixels: the size the viewer
/// renders at, and the origin the pointer lock needs to pin the cursor to its
/// centre. X11 and GPUI agree on this space; only GPUI's logical pixels differ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Viewport {
    pub(super) origin: (u32, u32),
    pub(super) size: (u32, u32),
}

impl Viewport {
    /// The centre the look gesture pins the pointer to, `None` before the first
    /// layout has given the panel a size.
    pub(super) fn centre(&self) -> Option<(i16, i16)> {
        if self.size.0 == 0 || self.size.1 == 0 {
            return None;
        }

        Some((
            coordinate(self.origin.0 + self.size.0 / 2),
            coordinate(self.origin.1 + self.size.1 / 2),
        ))
    }
}

/// X11 window coordinates are signed 16-bit; a viewport past that is already
/// wider than any display, so saturating is as good as failing.
fn coordinate(pixels: u32) -> i16 {
    i16::try_from(pixels).unwrap_or(i16::MAX)
}

pub(super) fn device_pixels(logical: Pixels, scale: f32) -> u32 {
    (f32::from(logical) * scale).round().max(0.0) as u32
}

/// Wraps a rendered frame in the image GPUI uploads. See
/// [`crate::render_image`] for why the colour channels need swapping.
pub(super) fn render_image(pixels: Vec<u8>, width: u32, height: u32) -> Option<Arc<RenderImage>> {
    to_render_image(pixels, width, height)
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::{device_pixels, Viewport};

    #[test]
    fn device_pixels_scale_logical_ones() {
        assert_eq!(device_pixels(px(100.0), 1.0), 100);
        assert_eq!(device_pixels(px(100.0), 1.5), 150);
        assert_eq!(device_pixels(px(0.0), 2.0), 0);
    }

    // render_image() is a thin wrapper: its behaviour (the BGRA swap, the
    // short-buffer rejection) is covered by crate::render_image's own tests.

    #[test]
    fn the_centre_is_the_middle_of_the_panel_not_of_the_window() {
        let viewport = Viewport {
            origin: (40, 60),
            size: (1000, 800),
        };

        assert_eq!(viewport.centre(), Some((540, 460)));
    }

    #[test]
    fn an_unlaid_out_viewport_has_no_centre_to_pin_to() {
        assert_eq!(Viewport::default().centre(), None);
        assert_eq!(
            Viewport {
                origin: (10, 10),
                size: (0, 800),
            }
            .centre(),
            None
        );
    }
}
