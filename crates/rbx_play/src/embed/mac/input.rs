//! Pointer and key events into the pinned player through Quartz events
//! posted where hardware events enter (`kCGHIDEventTap`), so the player
//! reads them as it reads real ones. Coordinates are the game's pixels:
//! the window below its title bar.

use std::thread;

use super::super::pin;
use super::{events, failed, focus_window, frame_and_scale};
use crate::embed::{Button, EmbedError, WindowId, CLICK_HOLD};
use crate::keycode::KeyCode;

/// `CGEventType`: left down/up, right down/up, moved.
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOVED: u32 = 5;

fn sent(ok: bool, what: &str) -> Result<(), EmbedError> {
    match ok {
        true => Ok(()),
        false => Err(failed(what)),
    }
}

/// One run of input into a player window.
pub struct Input {
    window: WindowId,
}

impl Input {
    pub fn open(window: WindowId) -> Result<Self, EmbedError> {
        Ok(Input { window })
    }

    /// The game's area in pixels.
    pub fn size(&self) -> Result<(u32, u32), EmbedError> {
        let (frame, scale) = frame_and_scale(self.window)?;
        Ok(pin::content_size(frame, scale))
    }

    /// Moves the cursor to `(x, y)` in the game's pixels.
    pub fn move_to(&mut self, x: i32, y: i32) -> Result<(), EmbedError> {
        let (frame, scale) = frame_and_scale(self.window)?;
        let (x, y) = pin::client_point(frame, scale, (x, y));
        sent(
            events::post_mouse(MOVED, events::CGPoint { x, y }, 0),
            "CGEventPost",
        )
    }

    pub fn button(&mut self, button: Button, down: bool) -> Result<(), EmbedError> {
        let (kind, number) = match (button, down) {
            (Button::Left, true) => (LEFT_DOWN, 0),
            (Button::Left, false) => (LEFT_UP, 0),
            (Button::Right, true) => (RIGHT_DOWN, 1),
            (Button::Right, false) => (RIGHT_UP, 1),
        };
        sent(
            events::post_mouse(kind, events::cursor(), number),
            "CGEventPost",
        )
    }

    /// One wheel notch where the cursor is.
    pub fn scroll(&mut self, up: bool) -> Result<(), EmbedError> {
        sent(events::post_scroll(if up { 1 } else { -1 }), "CGEventPost")?;
        thread::sleep(CLICK_HOLD);
        Ok(())
    }

    /// Presses or releases `key` by its place on a US keyboard
    /// ([`KeyCode::physical`]) as a Mac key code, with Shift around a
    /// shifted symbol. A name with no place types its character, if it has
    /// a printable one, on the press.
    pub fn key(&mut self, key: &KeyCode, down: bool) -> Result<(), EmbedError> {
        let no_key = || EmbedError::X11(format!("no key types {}", key.name));
        let Some(place) = key.physical() else {
            let c = char::from_u32(key.keysym)
                .filter(|c| (' '..='~').contains(c))
                .ok_or_else(no_key)?;
            return match down {
                true => self.type_text(&c.to_string()),
                false => Ok(()),
            };
        };
        let code = pin::mac_keycode(place.evdev).ok_or_else(no_key)?;
        focus_window(self.window)?;
        let shifted = place.shift && down;
        if shifted {
            sent(events::post_key(pin::MAC_SHIFT, true, true), "CGEventPost")?;
        }
        sent(events::post_key(code, down, shifted), "CGEventPost")?;
        if shifted {
            sent(
                events::post_key(pin::MAC_SHIFT, false, false),
                "CGEventPost",
            )?;
        }
        Ok(())
    }

    /// Types `text` as Unicode key events, for a text field a game script
    /// cannot reach (the chat bar).
    pub fn type_text(&mut self, text: &str) -> Result<(), EmbedError> {
        focus_window(self.window)?;
        let mut units = [0u16; 2];
        for c in text.chars() {
            sent(events::post_text(c.encode_utf16(&mut units)), "CGEventPost")?;
        }
        Ok(())
    }
}
