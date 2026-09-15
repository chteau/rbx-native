//! The X11 requests the pointer lock is made of, and the nothing it becomes
//! everywhere else.
//!
//! The connection is the lock's own, never GPUI's: warping and hiding are not
//! part of what GPUI's event loop does, and a second client connection costs one
//! socket and no synchronisation with it.

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
    pub(super) fn pointer(&self, window: u32) -> Option<(i16, i16)> {
        let reply = self.connection.query_pointer(window).ok()?.reply().ok()?;
        Some((reply.win_x, reply.win_y))
    }

    pub(super) fn warp(&self, window: u32, to: (i16, i16)) {
        // Failures are dropped throughout: a window already gone, or a server
        // that dislikes the request, must not take the camera down with it.
        let _ = self
            .connection
            .warp_pointer(x11rb::NONE, window, 0, 0, 0, 0, to.0, to.1);
        let _ = self.connection.flush();
    }

    pub(super) fn hide(&self, window: u32) {
        if self.hides {
            let _ = xfixes::hide_cursor(&self.connection, window);
            let _ = self.connection.flush();
        }
    }

    pub(super) fn show(&self, window: u32) {
        if self.hides {
            let _ = xfixes::show_cursor(&self.connection, window);
            let _ = self.connection.flush();
        }
    }
}

/// Never constructed: no other backend gets a lock, so every call below is
/// unreachable by construction rather than by convention.
#[cfg(not(target_os = "linux"))]
pub(super) enum Server {}

#[cfg(not(target_os = "linux"))]
impl Server {
    pub(super) fn open() -> Option<Self> {
        None
    }

    pub(super) fn pointer(&self, _window: u32) -> Option<(i16, i16)> {
        match *self {}
    }

    pub(super) fn warp(&self, _window: u32, _to: (i16, i16)) {
        match *self {}
    }

    pub(super) fn hide(&self, _window: u32) {
        match *self {}
    }

    pub(super) fn show(&self, _window: u32) {
        match *self {}
    }
}
