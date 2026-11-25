//! Play (local): the keyboard and mouse go to the game, as a client's do,
//! not to the free camera. They're queued here as `rbx_sim` inputs; the run
//! hands them to the game each frame, and the game's camera (our own
//! default one, or the place's scripts) decides where the view looks and
//! whether the mouse is locked.

use std::collections::HashSet;

use rbx_sim::{Input, MouseBehavior};

#[derive(Debug, Default)]
pub(crate) struct PlayInput {
    queued: Vec<Input>,
    /// Keys (as `KeyCode` names) and mouse buttons down, so a repeat isn't
    /// a second press and losing focus releases them.
    keys: HashSet<String>,
    buttons: HashSet<u8>,
    /// What the game asked of the mouse on the last frame.
    pub(crate) mouse: MouseBehavior,
}

/// The `Enum.KeyCode` of a GPUI key name. Letters go by where they sit on
/// a QWERTY keyboard, so an AZERTY player's Z Q S D walk like W A S D (Roblox
/// reads key positions the same way).
pub(crate) fn key_code(key: &str, azerty: bool) -> Option<String> {
    let named = match key {
        "space" => "Space",
        "enter" => "Return",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "escape" => "Escape",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "shift" => "LeftShift",
        "control" => "LeftControl",
        "alt" => "LeftAlt",
        "-" => "Minus",
        "=" => "Equals",
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "'" => "Quote",
        "[" => "LeftBracket",
        "]" => "RightBracket",
        "`" => "Backquote",
        "\\" => "BackSlash",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if let Some(f) = key.strip_prefix('f').filter(|n| n.parse::<u8>().is_ok()) {
        return Some(format!("F{f}"));
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    // A letter let go with Shift held can come back as its capital.
    let c = match (azerty, c.to_ascii_lowercase()) {
        (true, 'z') => 'w',
        (true, 'w') => 'z',
        (true, 'q') => 'a',
        (true, 'a') => 'q',
        (_, c) => c,
    };
    const DIGITS: [&str; 10] = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    match c {
        'a'..='z' => Some(c.to_ascii_uppercase().to_string()),
        '0'..='9' => Some(DIGITS[c as usize - '0' as usize].to_owned()),
        _ => None,
    }
}

impl PlayInput {
    /// A key pressed or released; `false` for one the game has no code for.
    pub(crate) fn key(&mut self, key: &str, pressed: bool, azerty: bool) -> bool {
        let Some(code) = key_code(key, azerty) else {
            return false;
        };
        self.code(code, pressed);
        true
    }

    /// Shift, which only ever arrives as a modifier change.
    pub(crate) fn shift(&mut self, held: bool) {
        self.code("LeftShift".to_owned(), held);
    }

    fn code(&mut self, code: String, pressed: bool) {
        let changed = if pressed {
            self.keys.insert(code.clone())
        } else {
            self.keys.remove(&code)
        };
        if changed {
            self.queued.push(Input::Key {
                code,
                down: pressed,
            });
        }
    }

    pub(crate) fn button(&mut self, button: u8, down: bool) {
        let changed = if down {
            self.buttons.insert(button)
        } else {
            self.buttons.remove(&button)
        };
        if changed {
            self.queued.push(Input::Button { button, down });
        }
    }

    /// A key pressed and let go at once (a hotbar slot clicked).
    pub(crate) fn tap(&mut self, code: &str) {
        for down in [true, false] {
            self.queued.push(Input::Key {
                code: code.to_owned(),
                down,
            });
        }
    }

    pub(crate) fn moved(&mut self, position: [f32; 2], delta: [f32; 2]) {
        self.queued.push(Input::Move { position, delta });
    }

    pub(crate) fn wheel(&mut self, notches: f32) {
        self.queued.push(Input::Wheel { notches });
    }

    /// Nothing stays held once the view loses the keyboard.
    pub(crate) fn release(&mut self) {
        for code in std::mem::take(&mut self.keys) {
            self.queued.push(Input::Key { code, down: false });
        }
        for button in std::mem::take(&mut self.buttons) {
            self.queued.push(Input::Button {
                button,
                down: false,
            });
        }
    }

    /// What happened since the last frame, in order.
    pub(crate) fn take(&mut self) -> Vec<Input> {
        std::mem::take(&mut self.queued)
    }

    /// Whether the game wants the pointer held (shift lock, first person,
    /// a right-drag).
    pub(crate) fn wants_lock(&self) -> bool {
        self.mouse != MouseBehavior::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_key_codes_by_position() {
        assert_eq!(key_code("w", false).as_deref(), Some("W"));
        assert_eq!(key_code("z", true).as_deref(), Some("W"));
        assert_eq!(key_code("q", true).as_deref(), Some("A"));
        assert_eq!(key_code("space", false).as_deref(), Some("Space"));
        assert_eq!(key_code("3", false).as_deref(), Some("Three"));
        assert_eq!(key_code("f5", false).as_deref(), Some("F5"));
        assert_eq!(key_code("é", true), None);
    }

    #[test]
    fn a_repeat_is_not_a_second_press_and_release_lets_go() {
        let mut input = PlayInput::default();
        assert!(input.key("w", true, false));
        assert!(input.key("w", true, false));
        input.button(2, true);
        input.release();
        assert_eq!(
            input.take(),
            vec![
                Input::Key {
                    code: "W".into(),
                    down: true
                },
                Input::Button {
                    button: 2,
                    down: true
                },
                Input::Key {
                    code: "W".into(),
                    down: false
                },
                Input::Button {
                    button: 2,
                    down: false
                },
            ]
        );
    }
}
