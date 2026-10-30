//! Roblox `Enum.KeyCode` names and the operating-system key each one is
//! typed with. The list is every name Studio's own `user_keyboard_input`
//! accepts, in its order.
//!
//! A `KeyCode` names a key by where it sits on a US keyboard, whatever the
//! layout types there (`UserInputService:GetStringForKeyCode` exists to say
//! what `Q` types on AZERTY: `A`), and the client reads keys that way too:
//! the key at W's place walks forward on any layout. So a key is sent by
//! its place ([`KeyCode::physical`]): an evdev code (an X11 keycode is that
//! plus 8) and a Windows set-1 scan code. Only the names no US key has on
//! its own (the shifted symbols) add Shift, and the few with no place at all
//! (`World0`.., `Euro`) fall back to the layout: an X11 keysym, a Win32
//! virtual-key code.
//!
//! A name with no key on a keyboard (gamepad buttons, mouse buttons and
//! axes, touch, Power) has `0` for both: Studio feeds those into the engine
//! directly, and the real client has no OS input that produces them. A few
//! keys X11 has a keysym for have no Win32 virtual key (`World0`..`World95`,
//! the Latin-1 keys; `KeypadEquals`, `Compose`, `Euro`, `Undo`): `vk` is `0`.

/// One `Enum.KeyCode` name and how a keyboard types it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCode {
    pub name: &'static str,
    /// The X11 keysym; `0` when no key types it.
    pub keysym: u32,
    /// The Win32 virtual-key code; `0` when no key types it.
    pub vk: u16,
    /// Win32 only: the key is on the extended part of the keyboard
    /// (`KEYEVENTF_EXTENDEDKEY`): arrows, Insert/Delete/Home/End/PageUp/
    /// PageDown, the right-hand Control and Alt, keypad Enter and Divide.
    pub extended: bool,
}

impl KeyCode {
    /// The US keyboard key it names; `None` for the names no key has a
    /// place for (`World0`.., `Euro`, `Undo`, gamepad and mouse names).
    pub fn physical(&self) -> Option<Physical> {
        physical(self.name)
    }

    /// Whether this platform's keyboard input can type it.
    pub fn typeable(&self) -> bool {
        if self.physical().is_some() {
            return true;
        }
        if cfg!(windows) {
            self.vk != 0
        } else {
            self.keysym != 0
        }
    }
}

/// Where a US keyboard has a key: its Linux evdev code (`KEY_*` in
/// `input-event-codes.h`), and whether Shift goes with it, as for `Hash`
/// (Shift+3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Physical {
    pub evdev: u16,
    pub shift: bool,
}

/// `KEY_LEFTSHIFT`.
pub const LEFT_SHIFT: u16 = 42;

impl Physical {
    /// The X11 keycode: evdev's plus 8, as the X server's evdev keymap
    /// numbers them.
    pub fn x11(self) -> u8 {
        u8::try_from(self.evdev + 8).unwrap_or(0)
    }

    /// The Windows set-1 scan code and whether it is an extended (`E0`)
    /// one; `None` for the few keys that have none `SendInput` can send
    /// (Pause, Help).
    pub fn scancode(self) -> Option<(u16, bool)> {
        set1(self.evdev)
    }
}

/// evdev numbers its first 88 keys by their set-1 scan codes; the rest are
/// mapped one by one.
fn set1(evdev: u16) -> Option<(u16, bool)> {
    Some(match evdev {
        1..=88 => (evdev, false),
        96 => (0x1c, true),
        97 => (0x1d, true),
        98 => (0x35, true),
        99 => (0x37, true),
        100 => (0x38, true),
        102 => (0x47, true),
        103 => (0x48, true),
        104 => (0x49, true),
        105 => (0x4b, true),
        106 => (0x4d, true),
        107 => (0x4f, true),
        108 => (0x50, true),
        109 => (0x51, true),
        110 => (0x52, true),
        111 => (0x53, true),
        117 => (0x59, false),
        125 => (0x5b, true),
        126 => (0x5c, true),
        127 => (0x5d, true),
        183..=185 => (evdev - 183 + 0x64, false),
        _ => return None,
    })
}

/// The US keyboard key Roblox reads as `name`.
fn physical(name: &str) -> Option<Physical> {
    let plain = |evdev| {
        Some(Physical {
            evdev,
            shift: false,
        })
    };
    let shifted = |evdev| Some(Physical { evdev, shift: true });
    if let [letter @ b'A'..=b'Z'] = name.as_bytes() {
        const LETTERS: [u16; 26] = [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47,
            17, 45, 21, 44,
        ];
        return plain(LETTERS[usize::from(letter - b'A')]);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return match n {
            1..=10 => plain(58 + n),
            11 | 12 => plain(76 + n),
            13..=15 => plain(170 + n),
            _ => None,
        };
    }
    const DIGITS: [&str; 10] = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    if let Some(n) = DIGITS.iter().position(|d| *d == name) {
        // 1..9 are evdev 2..10, 0 is 11.
        return plain(if n == 0 { 11 } else { n as u16 + 1 });
    }
    if let Some(n) = name
        .strip_prefix("Keypad")
        .and_then(|d| DIGITS.iter().position(|digit| *digit == d))
    {
        const KEYPAD: [u16; 10] = [82, 79, 80, 81, 75, 76, 77, 71, 72, 73];
        return plain(KEYPAD[n]);
    }
    match name {
        "Backspace" => plain(14),
        "Tab" => plain(15),
        "Return" => plain(28),
        "Pause" | "Break" => plain(119),
        "Escape" => plain(1),
        "Space" => plain(57),
        "Quote" => plain(40),
        "Comma" => plain(51),
        "Minus" => plain(12),
        "Period" => plain(52),
        "Slash" => plain(53),
        "Semicolon" => plain(39),
        "Equals" => plain(13),
        "LeftBracket" => plain(26),
        "BackSlash" => plain(43),
        "RightBracket" => plain(27),
        "Backquote" => plain(41),
        "Delete" => plain(111),
        "KeypadPeriod" => plain(83),
        "KeypadDivide" => plain(98),
        "KeypadMultiply" => plain(55),
        "KeypadMinus" => plain(74),
        "KeypadPlus" => plain(78),
        "KeypadEnter" => plain(96),
        "KeypadEquals" => plain(117),
        "Up" => plain(103),
        "Down" => plain(108),
        "Right" => plain(106),
        "Left" => plain(105),
        "Insert" => plain(110),
        "Home" => plain(102),
        "End" => plain(107),
        "PageUp" => plain(104),
        "PageDown" => plain(109),
        "NumLock" => plain(69),
        "CapsLock" => plain(58),
        "ScrollLock" => plain(70),
        "RightShift" => plain(54),
        "LeftShift" => plain(LEFT_SHIFT),
        "RightControl" => plain(97),
        "LeftControl" => plain(29),
        "RightAlt" => plain(100),
        "LeftAlt" => plain(56),
        "LeftMeta" | "LeftSuper" => plain(125),
        "RightMeta" | "RightSuper" => plain(126),
        "Menu" | "Compose" => plain(127),
        "Print" | "SysReq" => plain(99),
        "Help" => plain(138),
        "QuotedDouble" => shifted(40),
        "Hash" => shifted(4),
        "Dollar" => shifted(5),
        "Percent" => shifted(6),
        "Ampersand" => shifted(8),
        "LeftParenthesis" => shifted(10),
        "RightParenthesis" => shifted(11),
        "Asterisk" => shifted(9),
        "Plus" => shifted(13),
        "Colon" => shifted(39),
        "LessThan" => shifted(51),
        "GreaterThan" => shifted(52),
        "Question" => shifted(53),
        "At" => shifted(3),
        "Caret" => shifted(7),
        "Underscore" => shifted(12),
        "LeftCurly" => shifted(26),
        "Pipe" => shifted(43),
        "RightCurly" => shifted(27),
        "Tilde" => shifted(41),
        _ => None,
    }
}

/// The key code named `name`, exactly as `Enum.KeyCode` spells it.
pub fn find(name: &str) -> Option<&'static KeyCode> {
    KEY_CODES.iter().find(|key| key.name == name)
}

/// Every name, in Studio's order (`user_keyboard_input`'s `key_code` enum).
pub fn names() -> impl Iterator<Item = &'static str> {
    KEY_CODES.iter().map(|key| key.name)
}

const fn key(name: &'static str, keysym: u32, vk: u16) -> KeyCode {
    KeyCode {
        name,
        keysym,
        vk,
        extended: false,
    }
}

const fn extended(name: &'static str, keysym: u32, vk: u16) -> KeyCode {
    KeyCode {
        name,
        keysym,
        vk,
        extended: true,
    }
}

const fn none(name: &'static str) -> KeyCode {
    key(name, 0, 0)
}

/// Every Enum.KeyCode name Studio's user_keyboard_input accepts, in its order.
pub const KEY_CODES: &[KeyCode] = &[
    none("None"),
    key("Backspace", 0xff08, 0x08),
    key("Tab", 0xff09, 0x09),
    key("Clear", 0xff0b, 0x0c),
    key("Return", 0xff0d, 0x0d),
    key("Pause", 0xff13, 0x13),
    key("Escape", 0xff1b, 0x1b),
    key("Space", 0x20, 0x20),
    key("QuotedDouble", 0x22, 0xde),
    key("Hash", 0x23, 0x33),
    key("Dollar", 0x24, 0x34),
    key("Percent", 0x25, 0x35),
    key("Ampersand", 0x26, 0x37),
    key("Quote", 0x27, 0xde),
    key("LeftParenthesis", 0x28, 0x39),
    key("RightParenthesis", 0x29, 0x30),
    key("Asterisk", 0x2a, 0x38),
    key("Plus", 0x2b, 0xbb),
    key("Comma", 0x2c, 0xbc),
    key("Minus", 0x2d, 0xbd),
    key("Period", 0x2e, 0xbe),
    key("Slash", 0x2f, 0xbf),
    key("Zero", 0x30, 0x30),
    key("One", 0x31, 0x31),
    key("Two", 0x32, 0x32),
    key("Three", 0x33, 0x33),
    key("Four", 0x34, 0x34),
    key("Five", 0x35, 0x35),
    key("Six", 0x36, 0x36),
    key("Seven", 0x37, 0x37),
    key("Eight", 0x38, 0x38),
    key("Nine", 0x39, 0x39),
    key("Colon", 0x3a, 0xba),
    key("Semicolon", 0x3b, 0xba),
    key("LessThan", 0x3c, 0xbc),
    key("Equals", 0x3d, 0xbb),
    key("GreaterThan", 0x3e, 0xbe),
    key("Question", 0x3f, 0xbf),
    key("At", 0x40, 0x32),
    key("LeftBracket", 0x5b, 0xdb),
    key("BackSlash", 0x5c, 0xdc),
    key("RightBracket", 0x5d, 0xdd),
    key("Caret", 0x5e, 0x36),
    key("Underscore", 0x5f, 0xbd),
    key("Backquote", 0x60, 0xc0),
    key("A", 0x61, 0x41),
    key("B", 0x62, 0x42),
    key("C", 0x63, 0x43),
    key("D", 0x64, 0x44),
    key("E", 0x65, 0x45),
    key("F", 0x66, 0x46),
    key("G", 0x67, 0x47),
    key("H", 0x68, 0x48),
    key("I", 0x69, 0x49),
    key("J", 0x6a, 0x4a),
    key("K", 0x6b, 0x4b),
    key("L", 0x6c, 0x4c),
    key("M", 0x6d, 0x4d),
    key("N", 0x6e, 0x4e),
    key("O", 0x6f, 0x4f),
    key("P", 0x70, 0x50),
    key("Q", 0x71, 0x51),
    key("R", 0x72, 0x52),
    key("S", 0x73, 0x53),
    key("T", 0x74, 0x54),
    key("U", 0x75, 0x55),
    key("V", 0x76, 0x56),
    key("W", 0x77, 0x57),
    key("X", 0x78, 0x58),
    key("Y", 0x79, 0x59),
    key("Z", 0x7a, 0x5a),
    key("LeftCurly", 0x7b, 0xdb),
    key("Pipe", 0x7c, 0xdc),
    key("RightCurly", 0x7d, 0xdd),
    key("Tilde", 0x7e, 0xc0),
    extended("Delete", 0xffff, 0x2e),
    key("World0", 0xa0, 0),
    key("World1", 0xa1, 0),
    key("World2", 0xa2, 0),
    key("World3", 0xa3, 0),
    key("World4", 0xa4, 0),
    key("World5", 0xa5, 0),
    key("World6", 0xa6, 0),
    key("World7", 0xa7, 0),
    key("World8", 0xa8, 0),
    key("World9", 0xa9, 0),
    key("World10", 0xaa, 0),
    key("World11", 0xab, 0),
    key("World12", 0xac, 0),
    key("World13", 0xad, 0),
    key("World14", 0xae, 0),
    key("World15", 0xaf, 0),
    key("World16", 0xb0, 0),
    key("World17", 0xb1, 0),
    key("World18", 0xb2, 0),
    key("World19", 0xb3, 0),
    key("World20", 0xb4, 0),
    key("World21", 0xb5, 0),
    key("World22", 0xb6, 0),
    key("World23", 0xb7, 0),
    key("World24", 0xb8, 0),
    key("World25", 0xb9, 0),
    key("World26", 0xba, 0),
    key("World27", 0xbb, 0),
    key("World28", 0xbc, 0),
    key("World29", 0xbd, 0),
    key("World30", 0xbe, 0),
    key("World31", 0xbf, 0),
    key("World32", 0xc0, 0),
    key("World33", 0xc1, 0),
    key("World34", 0xc2, 0),
    key("World35", 0xc3, 0),
    key("World36", 0xc4, 0),
    key("World37", 0xc5, 0),
    key("World38", 0xc6, 0),
    key("World39", 0xc7, 0),
    key("World40", 0xc8, 0),
    key("World41", 0xc9, 0),
    key("World42", 0xca, 0),
    key("World43", 0xcb, 0),
    key("World44", 0xcc, 0),
    key("World45", 0xcd, 0),
    key("World46", 0xce, 0),
    key("World47", 0xcf, 0),
    key("World48", 0xd0, 0),
    key("World49", 0xd1, 0),
    key("World50", 0xd2, 0),
    key("World51", 0xd3, 0),
    key("World52", 0xd4, 0),
    key("World53", 0xd5, 0),
    key("World54", 0xd6, 0),
    key("World55", 0xd7, 0),
    key("World56", 0xd8, 0),
    key("World57", 0xd9, 0),
    key("World58", 0xda, 0),
    key("World59", 0xdb, 0),
    key("World60", 0xdc, 0),
    key("World61", 0xdd, 0),
    key("World62", 0xde, 0),
    key("World63", 0xdf, 0),
    key("World64", 0xe0, 0),
    key("World65", 0xe1, 0),
    key("World66", 0xe2, 0),
    key("World67", 0xe3, 0),
    key("World68", 0xe4, 0),
    key("World69", 0xe5, 0),
    key("World70", 0xe6, 0),
    key("World71", 0xe7, 0),
    key("World72", 0xe8, 0),
    key("World73", 0xe9, 0),
    key("World74", 0xea, 0),
    key("World75", 0xeb, 0),
    key("World76", 0xec, 0),
    key("World77", 0xed, 0),
    key("World78", 0xee, 0),
    key("World79", 0xef, 0),
    key("World80", 0xf0, 0),
    key("World81", 0xf1, 0),
    key("World82", 0xf2, 0),
    key("World83", 0xf3, 0),
    key("World84", 0xf4, 0),
    key("World85", 0xf5, 0),
    key("World86", 0xf6, 0),
    key("World87", 0xf7, 0),
    key("World88", 0xf8, 0),
    key("World89", 0xf9, 0),
    key("World90", 0xfa, 0),
    key("World91", 0xfb, 0),
    key("World92", 0xfc, 0),
    key("World93", 0xfd, 0),
    key("World94", 0xfe, 0),
    key("World95", 0xff, 0),
    key("KeypadZero", 0xffb0, 0x60),
    key("KeypadOne", 0xffb1, 0x61),
    key("KeypadTwo", 0xffb2, 0x62),
    key("KeypadThree", 0xffb3, 0x63),
    key("KeypadFour", 0xffb4, 0x64),
    key("KeypadFive", 0xffb5, 0x65),
    key("KeypadSix", 0xffb6, 0x66),
    key("KeypadSeven", 0xffb7, 0x67),
    key("KeypadEight", 0xffb8, 0x68),
    key("KeypadNine", 0xffb9, 0x69),
    key("KeypadPeriod", 0xffae, 0x6e),
    extended("KeypadDivide", 0xffaf, 0x6f),
    key("KeypadMultiply", 0xffaa, 0x6a),
    key("KeypadMinus", 0xffad, 0x6d),
    key("KeypadPlus", 0xffab, 0x6b),
    extended("KeypadEnter", 0xff8d, 0x0d),
    key("KeypadEquals", 0xffbd, 0),
    extended("Up", 0xff52, 0x26),
    extended("Down", 0xff54, 0x28),
    extended("Right", 0xff53, 0x27),
    extended("Left", 0xff51, 0x25),
    extended("Insert", 0xff63, 0x2d),
    extended("Home", 0xff50, 0x24),
    extended("End", 0xff57, 0x23),
    extended("PageUp", 0xff55, 0x21),
    extended("PageDown", 0xff56, 0x22),
    key("F1", 0xffbe, 0x70),
    key("F2", 0xffbf, 0x71),
    key("F3", 0xffc0, 0x72),
    key("F4", 0xffc1, 0x73),
    key("F5", 0xffc2, 0x74),
    key("F6", 0xffc3, 0x75),
    key("F7", 0xffc4, 0x76),
    key("F8", 0xffc5, 0x77),
    key("F9", 0xffc6, 0x78),
    key("F10", 0xffc7, 0x79),
    key("F11", 0xffc8, 0x7a),
    key("F12", 0xffc9, 0x7b),
    key("F13", 0xffca, 0x7c),
    key("F14", 0xffcb, 0x7d),
    key("F15", 0xffcc, 0x7e),
    key("NumLock", 0xff7f, 0x90),
    key("CapsLock", 0xffe5, 0x14),
    key("ScrollLock", 0xff14, 0x91),
    key("RightShift", 0xffe2, 0xa1),
    key("LeftShift", 0xffe1, 0xa0),
    extended("RightControl", 0xffe4, 0xa3),
    key("LeftControl", 0xffe3, 0xa2),
    extended("RightAlt", 0xffea, 0xa5),
    key("LeftAlt", 0xffe9, 0xa4),
    key("RightMeta", 0xffe8, 0x5c),
    key("LeftMeta", 0xffe7, 0x5b),
    extended("LeftSuper", 0xffeb, 0x5b),
    extended("RightSuper", 0xffec, 0x5c),
    key("Mode", 0xff7e, 0x1f),
    key("Compose", 0xff20, 0),
    key("Help", 0xff6a, 0x2f),
    key("Print", 0xff61, 0x2c),
    key("SysReq", 0xff15, 0x2c),
    key("Break", 0xff6b, 0x03),
    key("Menu", 0xff67, 0x5d),
    none("Power"),
    key("Euro", 0x20ac, 0),
    key("Undo", 0xff65, 0),
    none("ButtonX"),
    none("ButtonY"),
    none("ButtonA"),
    none("ButtonB"),
    none("ButtonR1"),
    none("ButtonL1"),
    none("ButtonR2"),
    none("ButtonL2"),
    none("ButtonR3"),
    none("ButtonL3"),
    none("ButtonStart"),
    none("ButtonSelect"),
    none("DPadLeft"),
    none("DPadRight"),
    none("DPadUp"),
    none("DPadDown"),
    none("Thumbstick1"),
    none("Thumbstick2"),
    none("Thumbstick1Up"),
    none("Thumbstick1Down"),
    none("Thumbstick1Left"),
    none("Thumbstick1Right"),
    none("Thumbstick2Up"),
    none("Thumbstick2Down"),
    none("Thumbstick2Left"),
    none("Thumbstick2Right"),
    none("MouseLeftButton"),
    none("MouseRightButton"),
    none("MouseMiddleButton"),
    none("MouseBackButton"),
    none("MouseNoButton"),
    none("MouseX"),
    none("MouseY"),
    none("MousePosition"),
    none("TouchPosition"),
    none("MouseWheel"),
    none("TrackpadPan"),
    none("TrackpadPinch"),
    none("MouseDelta"),
    none("TouchDelta"),
    none("TouchPinch"),
    none("ButtonCenter"),
    none("ButtonBack"),
    none("ButtonUp"),
    none("ButtonDown"),
    none("ButtonLeft"),
    none("ButtonRight"),
];

#[cfg(test)]
mod tests;
