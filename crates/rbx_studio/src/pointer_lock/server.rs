//! The X11 requests and Win32 calls the pointer lock is made of, and the
//! nothing it becomes everywhere else.
//!
//! On X11 the connection is the lock's own, never GPUI's: warping and hiding are
//! not part of what GPUI's event loop does, and a second client connection costs
//! one socket and no synchronisation with it.
//!
//! On Windows there is nothing to open. GPUI already calls `SetCapture` on
//! every button press, so moves keep arriving while the hidden pointer is off
//! the window between two warps; that is why no `ClipCursor` is taken either.

#[cfg(any(target_os = "linux", windows))]
use super::{At, WindowId};

#[cfg(target_os = "linux")]
use std::env;

#[cfg(target_os = "linux")]
use x11rb::connection::Connection as _;
#[cfg(target_os = "linux")]
use x11rb::protocol::xfixes;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::ConnectionExt as _;
#[cfg(target_os = "linux")]
use x11rb::rust_connection::RustConnection;

#[cfg(target_os = "linux")]
pub(super) struct Server {
    connection: RustConnection,
    /// Whether XFixes answered. Without it the pointer is still pinned, only
    /// visibly: a cursor that does not move is far less wrong than a look that
    /// escapes the viewport.
    hides: bool,
}

#[cfg(target_os = "linux")]
impl Server {
    pub(super) fn open() -> Option<Self> {
        let session = env::var("XDG_SESSION_TYPE").ok();
        let wayland = env::var("WAYLAND_DISPLAY").ok();
        if !super::x11_session(session.as_deref(), wayland.as_deref()) {
            return None;
        }

        let (connection, _) = x11rb::connect(None).ok()?;
        // XFixes refuses every request until a version has been negotiated;
        // hiding the cursor arrived in 4.0.
        let hides = xfixes::query_version(&connection, 4, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some();

        Some(Server { connection, hides })
    }

    /// The pointer's position relative to `window`, in physical pixels.
    pub(super) fn pointer(&self, window: WindowId) -> Option<At> {
        let reply = self.connection.query_pointer(window).ok()?.reply().ok()?;
        Some((reply.win_x, reply.win_y))
    }

    pub(super) fn warp(&self, window: WindowId, to: At) {
        // Failures are dropped throughout: a window already gone, or a server
        // that dislikes the request, must not take the camera down with it.
        let _ = self
            .connection
            .warp_pointer(x11rb::NONE, window, 0, 0, 0, 0, to.0, to.1);
        let _ = self.connection.flush();
    }

    pub(super) fn hide(&self, window: WindowId) {
        if self.hides {
            let _ = xfixes::hide_cursor(&self.connection, window);
            let _ = self.connection.flush();
        }
    }

    pub(super) fn show(&self, window: WindowId) {
        if self.hides {
            let _ = xfixes::show_cursor(&self.connection, window);
            let _ = self.connection.flush();
        }
    }
}

#[cfg(windows)]
pub(super) struct Server;

#[cfg(windows)]
impl Server {
    pub(super) fn open() -> Option<Self> {
        Some(Server)
    }

    /// The pointer's position relative to `window`'s client area, in physical
    /// pixels (GPUI makes the process per-monitor DPI aware).
    pub(super) fn pointer(&self, window: WindowId) -> Option<At> {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut at = POINT { x: 0, y: 0 };
        // SAFETY: both calls only write through the pointer they are handed,
        // and a stale `HWND` makes `ScreenToClient` fail rather than misbehave.
        let found =
            unsafe { GetCursorPos(&mut at) != 0 && ScreenToClient(hwnd(window), &mut at) != 0 };
        if !found {
            return None;
        }
        Some((i16::try_from(at.x).ok()?, i16::try_from(at.y).ok()?))
    }

    pub(super) fn warp(&self, window: WindowId, to: At) {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

        let mut at = POINT {
            x: to.0.into(),
            y: to.1.into(),
        };
        // SAFETY: as in `pointer`. Failures are dropped, as on X11.
        unsafe {
            if ClientToScreen(hwnd(window), &mut at) != 0 {
                SetCursorPos(at.x, at.y);
            }
        }
    }

    // `ShowCursor` moves a per-thread counter rather than setting a state, so
    // the two below must stay paired — which `PointerLock` guarantees by only
    // showing on the release of a look it hid for. Both run on GPUI's main
    // thread, the one that owns the window.
    pub(super) fn hide(&self, _window: WindowId) {
        // SAFETY: no pointers involved.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowCursor(0) };
    }

    pub(super) fn show(&self, _window: WindowId) {
        // SAFETY: no pointers involved.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowCursor(1) };
    }
}

#[cfg(windows)]
fn hwnd(window: WindowId) -> windows_sys::Win32::Foundation::HWND {
    window as windows_sys::Win32::Foundation::HWND
}

/// Never constructed: no other backend gets a lock, so every call below is
/// unreachable by construction rather than by convention.
#[cfg(not(any(target_os = "linux", windows)))]
pub(super) enum Server {}

#[cfg(not(any(target_os = "linux", windows)))]
impl Server {
    pub(super) fn open() -> Option<Self> {
        None
    }

    pub(super) fn pointer(&self, _window: super::WindowId) -> Option<super::At> {
        match *self {}
    }

    pub(super) fn warp(&self, _window: super::WindowId, _to: super::At) {
        match *self {}
    }

    pub(super) fn hide(&self, _window: super::WindowId) {
        match *self {}
    }

    pub(super) fn show(&self, _window: super::WindowId) {
        match *self {}
    }
}
