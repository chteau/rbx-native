//! Puts the Roblox client's window inside the editor's viewport, frameless,
//! and keeps it there: sized to the viewport, clipped around the editor's
//! popups, given the keyboard when clicked.
//!
//! - Linux (`x11`): re-parents the client's X11 window (Sober, or the
//!   official player under Wine) under the editor's window.
//! - Windows (`win32`): makes `RobloxPlayerBeta.exe`'s top-level window a
//!   `WS_CHILD` of the editor's `HWND`.
//! - macOS has no cross-process window parenting: the client keeps its own
//!   window there, and this module does not exist.
//!
//! Both back ends expose the same `Embedded` / `FocusFollow` API over
//! `WindowId`, the platform's native window handle.

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::*;

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use win32::*;

/// A native window: an X11 window id, or a Win32 `HWND` as an integer.
#[cfg(not(windows))]
pub type WindowId = u32;
#[cfg(windows)]
pub type WindowId = isize;

/// Pixel rect in the parent window's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum EmbedError {
    /// Could not open the X display (`$DISPLAY` unset, Wayland-only session).
    NoDisplay(String),
    /// No matching top-level window appeared within the timeout.
    NotFound,
    /// A windowing-system request failed (X11 or Win32); carries its
    /// description.
    X11(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedError::NoDisplay(e) => write!(f, "Could not open the X display: {e}."),
            EmbedError::NotFound => f.write_str("No Roblox client window appeared in time."),
            EmbedError::X11(e) => write!(f, "A window request failed: {e}."),
        }
    }
}

impl std::error::Error for EmbedError {}

/// `full` minus every hole, as non-overlapping rects (each hole splits the
/// remaining pieces into at most four: above, below, left, right). Holes
/// outside `full` change nothing; empty input pieces are dropped.
pub fn visible_parts(full: Rect, holes: &[Rect]) -> Vec<Rect> {
    let mut pieces = vec![full];
    for hole in holes {
        let (hx0, hy0) = (i64::from(hole.x), i64::from(hole.y));
        let (hx1, hy1) = (hx0 + i64::from(hole.width), hy0 + i64::from(hole.height));
        let mut next = Vec::with_capacity(pieces.len() + 3);
        for p in pieces {
            let (px0, py0) = (i64::from(p.x), i64::from(p.y));
            let (px1, py1) = (px0 + i64::from(p.width), py0 + i64::from(p.height));
            let (ix0, iy0) = (px0.max(hx0), py0.max(hy0));
            let (ix1, iy1) = (px1.min(hx1), py1.min(hy1));
            if ix0 >= ix1 || iy0 >= iy1 {
                next.push(p);
                continue;
            }
            for (x0, y0, x1, y1) in [
                (px0, py0, px1, iy0),
                (px0, iy1, px1, py1),
                (px0, iy0, ix0, iy1),
                (ix1, iy0, px1, iy1),
            ] {
                if x0 < x1 && y0 < y1 {
                    next.push(Rect {
                        x: x0 as i32,
                        y: y0 as i32,
                        width: (x1 - x0) as u32,
                        height: (y1 - y0) as u32,
                    });
                }
            }
        }
        pieces = next;
    }
    pieces
}
