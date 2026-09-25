//! Pointer capture while the camera looks around.
//!
//! GPUI exposes no pointer-lock API at all, so the lock is spoken straight to
//! the windowing system — the X server, or Win32: hide the cursor, then warp it
//! back to the viewport centre after every reported move. That is what keeps a
//! long look drag from wandering onto the Explorer and losing the rest of the
//! turn. Wayland's equivalent lives in a protocol GPUI does not forward, so
//! there the look keeps working exactly as it did, with a visible cursor and no
//! capture (see [`server`]).

mod server;

use gpui_kit::Window;
use raw_window_handle::RawWindowHandle;

use server::Server;

/// A pointer position inside the window GPUI drew, in physical pixels.
type At = (i16, i16);

/// The native window GPUI drew: an X11 window id, or a Win32 `HWND`.
#[cfg(not(windows))]
pub(crate) type WindowId = u32;
#[cfg(windows)]
pub(crate) type WindowId = isize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    window: WindowId,
    centre: At,
    /// Where the pointer sat when the look started, so releasing puts it back
    /// under the user's hand instead of in the middle of the viewport.
    restore: At,
}

/// The display-free half of the lock: which moves are real and which is the one
/// the warp itself caused. Split out so it can be tested without an X server.
#[derive(Debug, Default)]
struct Tracker {
    anchor: Option<Anchor>,
}

impl Tracker {
    fn start(&mut self, window: WindowId, centre: At, at: At) {
        self.anchor = Some(Anchor {
            window,
            centre,
            restore: at,
        });
    }

    /// How far `at` sits from the centre the pointer is pinned to, `None` for a
    /// move that lands exactly on it.
    ///
    /// Every warp makes the server report one more move, and it is
    /// indistinguishable from a real one except by position: filtering it out is
    /// the difference between a still view and one that drifts on its own.
    fn moved(&self, at: At) -> Option<(f32, f32)> {
        let anchor = self.anchor?;
        if at == anchor.centre {
            return None;
        }

        Some((
            f32::from(at.0 - anchor.centre.0),
            f32::from(at.1 - anchor.centre.1),
        ))
    }

    fn end(&mut self) -> Option<Anchor> {
        self.anchor.take()
    }
}

pub(crate) struct PointerLock {
    tracker: Tracker,
    /// `None` when this is neither an X11 session nor Windows, or when the
    /// display refused a second connection: the look then behaves as it did before any lock.
    server: Option<Server>,
}

impl PointerLock {
    pub(crate) fn new() -> Self {
        PointerLock {
            tracker: Tracker::default(),
            server: Server::open(),
        }
    }

    /// Hides the cursor and pins it to `centre`, in physical pixels relative to
    /// `window`, until [`PointerLock::release`]. Does nothing off X11 and
    /// Windows, and
    /// nothing at all if the pointer cannot be located.
    pub(crate) fn hold(&mut self, window: WindowId, centre: At) {
        let Some(server) = &self.server else {
            return;
        };
        let Some(at) = server.pointer(window) else {
            return;
        };

        self.tracker.start(window, centre, at);
        server.hide(window);
        server.warp(window, centre);
    }

    /// The travel since the last move, taken from the server rather than from
    /// the event: the position GPUI reports may already be a warp behind, and
    /// the delta has to be measured against the centre the pointer was pinned
    /// to. `None` while no look is held, and for the warp's own move.
    pub(crate) fn moved(&mut self) -> Option<(f32, f32)> {
        let server = self.server.as_ref()?;
        let anchor = self.tracker.anchor?;

        let delta = self.tracker.moved(server.pointer(anchor.window)?)?;
        server.warp(anchor.window, anchor.centre);
        Some(delta)
    }

    pub(crate) fn release(&mut self) {
        let Some(anchor) = self.tracker.end() else {
            return;
        };
        let Some(server) = &self.server else {
            return;
        };

        server.warp(anchor.window, anchor.restore);
        server.show(anchor.window);
    }

    pub(crate) fn holds(&self) -> bool {
        self.tracker.anchor.is_some()
    }
}

/// The X11 id or `HWND` of the window GPUI drew, `None` on every other backend.
pub(crate) fn window_id(window: &Window) -> Option<WindowId> {
    // GPUI's inherent `window_handle` is its own handle type, so the raw one has
    // to be asked for through the trait by name.
    let handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        #[cfg(not(windows))]
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        #[cfg(not(windows))]
        RawWindowHandle::Xlib(handle) => narrow_x11_id(handle.window),
        #[cfg(windows)]
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

/// Narrows a C `unsigned long` X id to the 32 bits the protocol gives one.
///
/// Xlib's `Window` is 64 bits wide on LP64 and 32 on Windows and 32-bit Unix, so
/// this is a real range check on the first and an identity on the rest. Written
/// over `TryInto` it is the same code on every target, where a concrete
/// `u32::try_from` is a same-type conversion clippy rejects wherever the widths
/// happen to match.
#[cfg(not(windows))]
fn narrow_x11_id<T: TryInto<u32>>(id: T) -> Option<u32> {
    id.try_into().ok()
}

/// Whether the session is one the X11 lock may talk to.
///
/// A Wayland session usually answers on `DISPLAY` too, through Xwayland, where
/// the warp would move a pointer the compositor does not follow — so it is ruled
/// out by the environment rather than by whether the connection succeeds.
#[cfg(target_os = "linux")]
fn x11_session(session: Option<&str>, wayland: Option<&str>) -> bool {
    if wayland.is_some_and(|display| !display.is_empty()) {
        return false;
    }

    session.is_none_or(|kind| kind.eq_ignore_ascii_case("x11"))
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::narrow_x11_id;
    #[cfg(target_os = "linux")]
    use super::x11_session;
    use super::{Tracker, WindowId};

    const WINDOW: WindowId = 0x42;
    const CENTRE: (i16, i16) = (750, 450);

    fn holding() -> Tracker {
        let mut tracker = Tracker::default();
        tracker.start(WINDOW, CENTRE, (20, 30));
        tracker
    }

    // The warp puts the pointer back on the centre, and the server reports that
    // as a move like any other. Turning the camera by it would double every
    // gesture and leave a still view drifting.
    #[test]
    fn the_warps_own_move_turns_nothing() {
        assert_eq!(holding().moved(CENTRE), None);
    }

    #[test]
    fn a_move_is_measured_from_the_centre() {
        let tracker = holding();
        assert_eq!(
            tracker.moved((CENTRE.0 + 7, CENTRE.1 - 3)),
            Some((7.0, -3.0))
        );
        assert_eq!(
            tracker.moved((CENTRE.0 - 120, CENTRE.1 + 40)),
            Some((-120.0, 40.0))
        );
    }

    // Each reported move is one leg of the gesture, never a position in a
    // gesture-long sum: the pointer is back at the centre before the next one.
    #[test]
    fn successive_moves_add_up_to_the_whole_travel() {
        let tracker = holding();
        let travel: f32 = [4, 9, -2, 11]
            .into_iter()
            .filter_map(|dx| tracker.moved((CENTRE.0 + dx, CENTRE.1)))
            .map(|(dx, _)| dx)
            .sum();

        assert_eq!(travel, 22.0);
    }

    #[test]
    fn releasing_hands_back_the_position_the_look_started_from() {
        let mut tracker = holding();
        let anchor = tracker.end().expect("a held look");

        assert_eq!(anchor.restore, (20, 30));
        assert_eq!(anchor.window, WINDOW);
        assert_eq!(tracker.end(), None);
        assert_eq!(tracker.moved((0, 0)), None);
    }

    // Xlib hands the id over as a C `unsigned long`, whatever width that is on
    // the target: a wide one that does not fit the protocol's 32 bits names no
    // window, and a narrow one always fits.
    #[cfg(not(windows))]
    #[test]
    fn an_xlib_id_is_narrowed_to_the_protocols_width() {
        assert_eq!(narrow_x11_id(WINDOW), Some(WINDOW));
        assert_eq!(narrow_x11_id(u64::from(WINDOW)), Some(WINDOW));
        assert_eq!(narrow_x11_id(u64::from(u32::MAX)), Some(u32::MAX));
        assert_eq!(narrow_x11_id(u64::from(u32::MAX) + 1), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_an_x11_session_is_locked() {
        assert!(x11_session(Some("x11"), None));
        assert!(x11_session(Some("X11"), Some("")));
        // No session type at all: a bare `DISPLAY`, as under a plain X server.
        assert!(x11_session(None, None));

        assert!(!x11_session(Some("wayland"), Some("wayland-0")));
        assert!(!x11_session(Some("x11"), Some("wayland-0")));
        assert!(!x11_session(Some("tty"), None));
    }
}
