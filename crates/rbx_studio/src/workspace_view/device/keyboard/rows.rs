//! The keys each page shows, per layout: the rows every style shares.
//! iOS and Gboard put the same letters in the same places; the styles
//! differ in what frames them (`iphone`, `fitted`).

use crate::device_sim::KeyboardLayout;

pub(super) fn letter_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    match layout {
        KeyboardLayout::Qwerty => [
            &["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
            &["a", "s", "d", "f", "g", "h", "j", "k", "l"],
            &["z", "x", "c", "v", "b", "n", "m"],
        ],
        // iOS's French AZERTY: the apostrophe takes the seventh key of the
        // bottom row, M moves up beside L.
        KeyboardLayout::Azerty => [
            &["a", "z", "e", "r", "t", "y", "u", "i", "o", "p"],
            &["q", "s", "d", "f", "g", "h", "j", "k", "l", "m"],
            &["w", "x", "c", "v", "b", "n", "\u{2019}"],
        ],
    }
}

pub(super) fn number_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    let second: &'static [&'static str] = match layout {
        KeyboardLayout::Qwerty => &["-", "/", ":", ";", "(", ")", "$", "&", "@", "\""],
        KeyboardLayout::Azerty => &["-", "/", ":", ";", "(", ")", "\u{20ac}", "&", "@", "\""],
    };
    [
        &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
        second,
        &[".", ",", "?", "!", "\u{2019}"],
    ]
}

pub(super) fn symbol_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    let second: &'static [&'static str] = match layout {
        KeyboardLayout::Qwerty => &[
            "_", "\\", "|", "~", "<", ">", "\u{20ac}", "\u{a3}", "\u{a5}", "\u{2022}",
        ],
        KeyboardLayout::Azerty => &[
            "_", "\\", "|", "~", "<", ">", "$", "\u{a3}", "\u{a5}", "\u{2022}",
        ],
    };
    [
        &["[", "]", "{", "}", "#", "%", "^", "*", "+", "="],
        second,
        &[".", ",", "?", "!", "\u{2019}"],
    ]
}
