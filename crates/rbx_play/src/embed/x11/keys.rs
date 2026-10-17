//! Typing a key into the embedded client: the Device Simulator's Android
//! back button.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

use super::{x11, Embedded};
use crate::embed::EmbedError;

/// X11's `XK_Escape`.
const ESCAPE_KEYSYM: u32 = 0xff1b;
/// How long the pointer stays over the client: 100 ms was enough in a
/// live test, returning it at once lost the key.
const POINTER_RETURN: std::time::Duration = std::time::Duration::from_millis(150);

impl Embedded {
    /// Types Escape into the window with XTest — a real key event, which
    /// clients accept where they drop `SendEvent` ones. XTest types into
    /// whatever has the focus, hence the focus first, and Sober reads the
    /// keyboard only while the pointer is over it (measured: focus alone, or
    /// moving the pointer back at once, loses the key), so the pointer is
    /// put in the middle of the window and back where it was a moment later.
    pub fn press_escape(&self) -> Result<(), EmbedError> {
        use x11rb::protocol::xproto::{KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
        use x11rb::protocol::xtest::ConnectionExt as _;

        let keycode = self.keycode(ESCAPE_KEYSYM)?;
        let pointer = self
            .conn
            .query_pointer(self.root)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let geometry = self
            .conn
            .get_geometry(self.window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let middle = |extent: u16| i16::try_from(extent / 2).unwrap_or(i16::MAX);
        self.conn
            .warp_pointer(
                x11rb::NONE,
                self.window,
                0,
                0,
                0,
                0,
                middle(geometry.width),
                middle(geometry.height),
            )
            .map_err(x11)?;
        self.focus()?;
        for kind in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
            self.conn
                .xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, self.root, 0, 0, 0)
                .map_err(x11)?;
        }
        self.conn.flush().map_err(x11)?;
        let (x, y) = (pointer.root_x, pointer.root_y);
        std::thread::spawn(move || {
            std::thread::sleep(POINTER_RETURN);
            if let Ok((conn, root)) = super::connect() {
                let _ = conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, x, y);
                let _ = conn.flush();
            }
        });
        Ok(())
    }

    /// The first keycode the server maps `keysym` to.
    fn keycode(&self, keysym: u32) -> Result<u8, EmbedError> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        map.keysyms
            .chunks(per)
            .position(|syms| syms.contains(&keysym))
            .and_then(|at| u8::try_from(usize::from(min) + at).ok())
            .ok_or_else(|| EmbedError::X11("no keycode for Escape".to_owned()))
    }
}
