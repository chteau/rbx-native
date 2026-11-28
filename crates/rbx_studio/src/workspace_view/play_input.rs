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

/// The `Enum.KeyCode` of a GPUI key name. A `KeyCode` names a key by where
/// it sits on a US keyboard, whatever the layout types there: measured in
/// Studio's Play on a French layout, the key typing `&` is `One` with or
/// without Shift, `a` is `Q`, `m` is `Semicolon` and `!` is `Slash`. GPUI
/// reports the character typed, not the key, so the place is worked back
/// from the character and the layout: a shifted character (`!` on US, `1`
/// on AZERTY) gives its key too.
pub(crate) fn key_code(key: &str, azerty: bool) -> Option<String> {
    let named = match key {
        "space" => "Space",
        "enter" => "Return",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "escape" => "Escape",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "shift" => "LeftShift",
        "control" => "LeftControl",
        "alt" => "LeftAlt",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if let Some(f) = key.strip_prefix('f').filter(|n| n.parse::<u8>().is_ok()) {
        return Some(format!("F{f}"));
    }
    let position = if azerty {
        azerty_position(key)
    } else {
        us_position(key)
    };
    position.map(str::to_owned)
}

const DIGITS: [&str; 10] = [
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
];

/// A letter's own key, either case (one let go with Shift held can come
/// back as its capital).
fn letter(key: &str) -> Option<char> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase()),
        _ => None,
    }
}

fn letter_code(c: char) -> &'static str {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    LETTERS[(c as u8 - b'A') as usize]
}

/// The US key typing `key`, unshifted or shifted.
fn us_position(key: &str) -> Option<&'static str> {
    if let Some(c) = letter(key) {
        return Some(letter_code(c));
    }
    Some(match key {
        "0" | ")" => DIGITS[0],
        "1" | "!" => DIGITS[1],
        "2" | "@" => DIGITS[2],
        "3" | "#" => DIGITS[3],
        "4" | "$" => DIGITS[4],
        "5" | "%" => DIGITS[5],
        "6" | "^" => DIGITS[6],
        "7" | "&" => DIGITS[7],
        "8" | "*" => DIGITS[8],
        "9" | "(" => DIGITS[9],
        "-" | "_" => "Minus",
        "=" | "+" => "Equals",
        "[" | "{" => "LeftBracket",
        "]" | "}" => "RightBracket",
        "\\" | "|" => "BackSlash",
        ";" | ":" => "Semicolon",
        "'" | "\"" => "Quote",
        "," | "<" => "Comma",
        "." | ">" => "Period",
        "/" | "?" => "Slash",
        "`" | "~" => "Backquote",
        _ => return None,
    })
}

/// The US key at the place of the French AZERTY key typing `key`,
/// unshifted or shifted. Its non-ASCII characters come as themselves
/// (Windows, macOS) or as their X keysym names (Linux). The key between
/// left Shift and W, `<`, has no US place: Studio reads it as
/// `KeyCode.None`, so it gives nothing here.
fn azerty_position(key: &str) -> Option<&'static str> {
    if let Some(c) = letter(key) {
        return Some(match c {
            'A' => "Q",
            'Q' => "A",
            'Z' => "W",
            'W' => "Z",
            'M' => "Semicolon",
            c => letter_code(c),
        });
    }
    Some(match key {
        "&" | "1" => DIGITS[1],
        "é" | "eacute" | "2" => DIGITS[2],
        "\"" | "3" => DIGITS[3],
        "'" | "4" => DIGITS[4],
        "(" | "5" => DIGITS[5],
        "-" | "6" => DIGITS[6],
        "è" | "egrave" | "7" => DIGITS[7],
        "_" | "8" => DIGITS[8],
        "ç" | "ccedilla" | "9" => DIGITS[9],
        "à" | "agrave" | "0" => DIGITS[0],
        ")" | "°" | "degree" => "Minus",
        "=" | "+" => "Equals",
        "^" | "dead_circumflex" | "¨" | "dead_diaeresis" => "LeftBracket",
        "$" | "dollar" | "£" | "sterling" => "RightBracket",
        "ù" | "ugrave" | "%" => "Quote",
        "*" | "µ" | "mu" => "BackSlash",
        "," | "?" => "M",
        ";" | "." => "Comma",
        ":" | "/" => "Period",
        "!" | "§" | "section" => "Slash",
        "²" | "twosuperior" => "Backquote",
        _ => return None,
    })
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

    /// Characters typed or pasted, for the game's focused TextBox. Empty,
    /// it tells the game up front that text comes this way: the platform
    /// sends a key's text after the key, and a box must not type the key's
    /// US character first.
    pub(crate) fn text(&mut self, text: &str) {
        self.queued.push(Input::Text {
            text: text.to_owned(),
        });
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
        assert_eq!(key_code("W", false).as_deref(), Some("W"));
        assert_eq!(key_code("space", false).as_deref(), Some("Space"));
        assert_eq!(key_code("3", false).as_deref(), Some("Three"));
        assert_eq!(key_code("f5", false).as_deref(), Some("F5"));
        // Shift held: the US digit row's symbols are still its digits.
        assert_eq!(key_code("!", false).as_deref(), Some("One"));
        assert_eq!(key_code("?", false).as_deref(), Some("Slash"));
    }

    /// Every French AZERTY key Studio's Play was pressed with, and the
    /// `KeyCode` it printed, unshifted and (digits) shifted.
    #[test]
    fn azerty_keys_read_like_studio_s() {
        let measured = [
            ("&", "One"),
            ("1", "One"),
            ("é", "Two"),
            ("eacute", "Two"),
            ("ç", "Nine"),
            ("ccedilla", "Nine"),
            ("à", "Zero"),
            ("agrave", "Zero"),
            (")", "Minus"),
            ("=", "Equals"),
            ("a", "Q"),
            ("z", "W"),
            ("q", "A"),
            ("w", "Z"),
            ("dead_circumflex", "LeftBracket"),
            ("$", "RightBracket"),
            ("m", "Semicolon"),
            ("*", "BackSlash"),
            (",", "M"),
            (";", "Comma"),
            (":", "Period"),
            ("/", "Period"),
            ("!", "Slash"),
        ];
        for (key, code) in measured {
            assert_eq!(key_code(key, true).as_deref(), Some(code), "{key}");
        }
        assert_eq!(key_code("<", true), None);
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
