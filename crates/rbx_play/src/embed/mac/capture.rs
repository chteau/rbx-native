//! `Embedded::capture`: the running game's pixels, for `screen_capture`.
//!
//! `CGWindowListCreateImage` reads the player's window from the window
//! server, so an editor window over it does not show; its title bar is cut
//! off. macOS returns only the desktop behind it until the editor has the
//! Screen Recording permission.

use super::super::pin;
use super::{failed, ffi, EmbedError, Embedded};

impl Embedded {
    /// The game's area of the window as `(width, height, RGBA)`.
    pub fn capture(&self) -> Result<(u32, u32, Vec<u8>), EmbedError> {
        let window = self.window();
        let info = ffi::window(window).ok_or(EmbedError::NotFound)?;
        let (width, height, bytes_per_row, bytes) =
            ffi::window_image(window).ok_or_else(|| failed("CGWindowListCreateImage"))?;
        if width == 0 || height == 0 || info.frame.height <= 0.0 {
            return Err(failed("capture of an empty window"));
        }
        let scale = height as f64 / info.frame.height;
        let top = ((pin::TITLE_BAR * scale).round() as usize).min(height);
        let rgba = pin::crop_rows(&bytes, bytes_per_row, width, height, top);
        let rows = u32::try_from(height - top).map_err(|_| failed("capture size"))?;
        let width = u32::try_from(width).map_err(|_| failed("capture size"))?;
        Ok((width, rows, rgba))
    }
}
