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
    dispatch(Box::new(done));
}

#[cfg(target_os = "linux")]
fn dispatch(done: Box<dyn FnOnce(Outcome) + Send>) {
    linux::pick(done);
}

#[cfg(windows)]
fn dispatch(done: Box<dyn FnOnce(Outcome) + Send>) {
    windows::pick(done);
}

#[cfg(target_os = "macos")]
fn dispatch(done: Box<dyn FnOnce(Outcome) + Send>) {
    macos::pick(done);
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn dispatch(done: Box<dyn FnOnce(Outcome) + Send>) {
    done(Err(
        "No screen colour picker is available on this system.".into()
    ));
}

/// The portal's 0–1 float channels, clamped and rounded.
pub fn from_unit(r: f64, g: f64, b: f64) -> Rgb8 {
    let to_byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [to_byte(r), to_byte(g), to_byte(b)]
}

/// One X11 `GetImage` ZPixmap pixel, given its visual's channel masks
/// (any contiguous mask of up to 8 bits; wider masks keep the top 8).
/// `None` if a mask is zero.
pub fn from_masked_pixel(
    pixel: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
) -> Option<Rgb8> {
    Some([
        scale_channel(pixel, red_mask)?,
        scale_channel(pixel, green_mask)?,
        scale_channel(pixel, blue_mask)?,
    ])
}

/// Extracts one contiguous channel and maps it onto 0–255. Narrow channels
/// are stretched so that their full range reaches 255 rather than a value
/// short of it; wide channels are truncated to their top eight bits.
fn scale_channel(pixel: u32, mask: u32) -> Option<u8> {
    if mask == 0 {
        return None;
    }
    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let value = u64::from((pixel & mask) >> shift);
    let max = (1u64 << bits) - 1;
    let scaled = if bits >= 8 {
        value >> (bits - 8)
    } else {
        value * 255 / max
    };
    Some(scaled as u8)
}

/// A Win32 `COLORREF` (`0x00BBGGRR`).
pub fn from_colorref(color: u32) -> Rgb8 {
    [color as u8, (color >> 8) as u8, (color >> 16) as u8]
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;
