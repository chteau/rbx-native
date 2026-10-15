//! Picking a colour from anywhere on screen. GUI toolkits like GPUI cannot
//! read the screen, so each platform asks its own:
//!
//! - Linux: the desktop portal's `Screenshot.PickColor` (GNOME, KDE and
//!   wlroots portals; works under Wayland), falling back on X11 to grabbing
//!   the pointer with a crosshair and reading the clicked pixel off the root
//!   window. Escape or a right click cancels.
//! - Windows: a low-level mouse hook swallows the next click and reads the
//!   pixel under it from the screen DC; Escape or a right click cancels.
//! - macOS: AppKit's `NSColorSampler`, the system loupe.

/// 8-bit sRGB.
pub type Rgb8 = [u8; 3];

/// The outcome: `Ok(None)` when the user backed out; `Err` is a sentence a
/// tooltip can show ("No screen colour picker is available on this
/// desktop.").
pub type Outcome = Result<Option<Rgb8>, String>;

/// Starts a pick and calls `done` once with the outcome. On Linux and
/// Windows the pick runs on its own thread and `done` is called there. On
/// macOS this must be called on the main thread, and `done` is called on
/// it.
pub fn pick(done: impl FnOnce(Outcome) + Send + 'static) {
    todo!()
}

/// The portal's 0–1 float channels, clamped and rounded.
pub fn from_unit(r: f64, g: f64, b: f64) -> Rgb8 {
    todo!()
}

/// One X11 `GetImage` ZPixmap pixel, given its visual's channel masks
/// (any contiguous mask of up to 8 bits; wider masks keep the top 8).
/// `None` if a mask is zero.
pub fn from_masked_pixel(pixel: u32, red_mask: u32, green_mask: u32, blue_mask: u32) -> Option<Rgb8> {
    todo!()
}

/// A Win32 `COLORREF` (`0x00BBGGRR`).
pub fn from_colorref(color: u32) -> Rgb8 {
    todo!()
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;
