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
    /// The panel at `origin`, `size` across, or, emulating a `screen`, the
    /// largest box of that screen's shape centred in it, the rest left as
    /// bars: the view a device of that shape would have, at the size the
    /// panel can show it. The scene renders at this box's size, 1:1 with
    /// the pixels it is shown in, so every pointer mapping stays as it was;
    /// only the GUI is laid out at the screen's own size (see
    /// `Headless::set_gui_screen`). `max_scale`, in physical pixels per
    /// screen unit, caps the box for display scaling; it still shrinks to
    /// fit.
    pub(super) fn letterboxed(
        origin: (u32, u32),
        size: (u32, u32),
        screen: Option<(u32, u32)>,
        max_scale: Option<f64>,
    ) -> Viewport {
        let Some((w, h)) = screen.filter(|&(w, h)| w > 0 && h > 0) else {
            return Viewport { origin, size };
        };
        let fit = (size.0 as f64 / w as f64).min(size.1 as f64 / h as f64);
        let scale = max_scale.map_or(fit, |cap| fit.min(cap));
        let fitted = (
            ((w as f64 * scale).round() as u32).clamp(1, size.0.max(1)),
            ((h as f64 * scale).round() as u32).clamp(1, size.1.max(1)),
        );
        Viewport {
            origin: (
                origin.0 + (size.0.saturating_sub(fitted.0)) / 2,
                origin.1 + (size.1.saturating_sub(fitted.1)) / 2,
            ),
            size: fitted,
        }
    }

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

/// How much `blurred` shrinks a frame before blurring it: GPUI's sampler
/// scales it back up, which is most of the softening for free.
const BLUR_STEP: usize = 8;

/// A soft, small copy of `image` (the last frame) to sit behind a veil:
/// shrunk by [`BLUR_STEP`], then three box-blur passes. Drawn stretched to
/// the panel it looks like a frosted copy of the scene.
pub(super) fn blurred(image: &RenderImage) -> Option<Arc<RenderImage>> {
    let bytes = image.as_bytes(0)?;
    let size = image.size(0);
    let (width, height) = (
        usize::try_from(size.width.0).ok()?,
        usize::try_from(size.height.0).ok()?,
    );
    let pixels = blur_pixels(bytes, width, height)?;
    let (small_w, small_h) = shrunk(width, height);
    // Already in GPUI's channel order: the bytes came from a RenderImage.
    let buffer = image::RgbaImage::from_raw(small_w as u32, small_h as u32, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

fn shrunk(width: usize, height: usize) -> (usize, usize) {
    ((width / BLUR_STEP).max(1), (height / BLUR_STEP).max(1))
}

/// `bytes` (4 per pixel, row-major) shrunk and blurred, as 4-byte pixels.
fn blur_pixels(bytes: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || bytes.len() < width * height * 4 {
        return None;
    }
    let (w, h) = shrunk(width, height);
    let mut small = vec![0f32; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 4];
            let mut count = 0f32;
            for sy in y * BLUR_STEP..((y + 1) * BLUR_STEP).min(height) {
                for sx in x * BLUR_STEP..((x + 1) * BLUR_STEP).min(width) {
                    let at = (sy * width + sx) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += f32::from(bytes[at + channel]);
                    }
                    count += 1.0;
                }
            }
            let at = (y * w + x) * 4;
            for (channel, total) in sum.into_iter().enumerate() {
                small[at + channel] = total / count.max(1.0);
            }
        }
    }
    for _ in 0..3 {
        small = box_pass(&small, w, h, true);
        small = box_pass(&small, w, h, false);
    }
    Some(
        small
            .into_iter()
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
            .collect(),
    )
}

/// One box-blur pass of radius 2 along rows (`horizontal`) or columns,
/// clamping at the edges.
fn box_pass(pixels: &[f32], w: usize, h: usize, horizontal: bool) -> Vec<f32> {
    const RADIUS: isize = 2;
    let mut out = vec![0f32; pixels.len()];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 4];
            for d in -RADIUS..=RADIUS {
                let (sx, sy) = match horizontal {
                    true => ((x as isize + d).clamp(0, w as isize - 1) as usize, y),
                    false => (x, (y as isize + d).clamp(0, h as isize - 1) as usize),
                };
                let at = (sy * w + sx) * 4;
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += pixels[at + channel];
                }
            }
            let at = (y * w + x) * 4;
            for (channel, total) in sum.into_iter().enumerate() {
                out[at + channel] = total / (2 * RADIUS + 1) as f32;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_flat_frame_stays_flat_and_shrinks_by_the_step() {
        let bytes = [10u8, 20, 30, 255].repeat(64 * 32);
        let out = super::blur_pixels(&bytes, 64, 32).unwrap();
        assert_eq!(out.len(), 8 * 4 * 4);
        assert!(out.chunks(4).all(|p| p == [10, 20, 30, 255]));
    }

    #[test]
    fn a_short_buffer_is_refused() {
        assert!(super::blur_pixels(&[0; 8], 4, 4).is_none());
    }

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

#[cfg(test)]
mod letterbox_tests {
    use super::Viewport;

    #[test]
    fn an_emulated_screen_is_the_largest_box_of_its_shape_centred_in_the_panel() {
        let native = Viewport::letterboxed((10, 20), (1000, 800), None, None);
        assert_eq!(
            native,
            Viewport {
                origin: (10, 20),
                size: (1000, 800)
            }
        );
        // 16:9 in a taller panel: full width, bars above and below.
        let wide = Viewport::letterboxed((10, 20), (1000, 800), Some((1920, 1080)), None);
        assert_eq!(
            wide,
            Viewport {
                origin: (10, 138),
                size: (1000, 563)
            }
        );
        // A portrait phone: full height, bars at the sides.
        let tall = Viewport::letterboxed((0, 0), (1000, 800), Some((390, 844)), None);
        assert_eq!(tall.size.1, 800);
        assert_eq!(tall.origin.0, (1000 - tall.size.0) / 2);
    }

    #[test]
    fn a_scale_cap_below_the_fit_shrinks_the_box_and_centres_it() {
        let v = Viewport::letterboxed((0, 0), (1000, 800), Some((100, 50)), Some(2.0));
        assert_eq!(
            v,
            Viewport {
                origin: (400, 350),
                size: (200, 100)
            }
        );
    }

    #[test]
    fn a_scale_cap_above_the_fit_still_fits() {
        let v = Viewport::letterboxed((0, 0), (1000, 800), Some((100, 50)), Some(50.0));
        assert_eq!(v.size, (1000, 500));
    }
}
