//! Typing a key into the embedded client: the Device Simulator's Android
//! back button.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

use super::{x11, Embedded};
use crate::embed::EmbedError;

/// X11's `XK_Escape`.
const ESCAPE_KEYSYM: u32 = 0xff1b;

impl Embedded {
    /// Focuses the window and types Escape into it with XTest — a real key
    /// event, which clients accept where they drop `SendEvent` ones. XTest
    /// types into whatever has the focus, hence the focus first.
    pub fn press_escape(&self) -> Result<(), EmbedError> {
        use x11rb::protocol::xproto::{KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
        use x11rb::protocol::xtest::ConnectionExt as _;

        let keycode = self.keycode(ESCAPE_KEYSYM)?;
        self.focus()?;
        for kind in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
            self.conn
                .xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, self.root, 0, 0, 0)
                .map_err(x11)?;
        }
        self.conn.flush().map_err(x11)
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
