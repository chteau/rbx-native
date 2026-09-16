//! Translating GPUI's window events into the viewer's camera input.
//!
//! The windowed viewer reads physical key codes straight from winit, which is
//! what makes WASD land on ZQSD on a French keyboard. GPUI 0.3.4's `Keystroke`
//! carries no key code at all — only the character the layout produced — so the
//! same physical mapping has to be rebuilt from that character plus the layout
//! GPUI reports (see [`Layout`]).

use gpui_kit::{Modifiers, ScrollDelta};
use rbx_viewer::CameraKey;

// GPUI's Linux backends report one wheel notch as three "lines" (their own
// `SCROLL_LINES`), not one, so notches have to be divided back out or a single
// notch would triple both the zoom step and the speed change.
const LINES_PER_NOTCH: f32 = 3.0;
// Touchpads and high-resolution wheels report pixels instead. No OS-independent
// notch size exists, so this is a documented approximation: GPUI's own line
// height of about 20 px, times the three lines a notch is worth.
const PIXELS_PER_NOTCH: f32 = 60.0;

/// Which letters the keyboard puts under the WASD positions.
///
/// AZERTY swaps two pairs: the W position types `z` and the A position types
/// `q`. `z`, `w` and the arrows are unambiguous (no layout uses them for
/// anything else here), but `a` and `q` mean opposite things on the two
/// layouts, so they are resolved with the layout rather than guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Qwerty,
    Azerty,
}

impl Layout {
    /// Classifies the layout name GPUI reports (`App::keyboard_layout`), which
    /// on Linux is the xkb name of the group in effect — "French", "Belgian",
    /// "English (US)".
    pub(crate) fn of(name: &str) -> Self {
        let name = name.to_lowercase();
        // French BÉPO is named after French but lays out like Dvorak, so its A
        // position types `a` exactly as QWERTY does.
        let dvorak_like = name.contains("bepo") || name.contains("dvorak");
        let azerty = ["azerty", "french", "belgian"]
            .iter()
            .any(|family| name.contains(family));

        if azerty && !dvorak_like {
            Layout::Azerty
        } else {
            Layout::Qwerty
        }
    }
}

/// Which camera key a keystroke drives, `None` for one the camera ignores.
///
/// `key` is `Keystroke::key`: a single lowercase character for letters, a name
/// like `"left"` for the arrows.
pub(crate) fn camera_key(key: &str, layout: Layout) -> Option<CameraKey> {
    let azerty = layout == Layout::Azerty;
    match key {
        "w" | "z" | "up" => Some(CameraKey::Forward),
        "s" | "down" => Some(CameraKey::Back),
        "d" | "right" => Some(CameraKey::Right),
        "left" => Some(CameraKey::Left),
        "e" => Some(CameraKey::Up),
        "a" if azerty => Some(CameraKey::Down),
        "a" => Some(CameraKey::Left),
        "q" if azerty => Some(CameraKey::Left),
        "q" => Some(CameraKey::Down),
        _ => None,
    }
}

/// The key a keystroke means to the transform toolbar, whose shortcuts are
/// the digits `1`-`4` — on an AZERTY keyboard those sit on the *shifted*
/// digit row, and the unshifted keys type `&`, `é`, `"` and `'` instead, which
/// GPUI reports under their keysym names. Studio binds the physical key, so
/// the unshifted row has to reach the same tools; `Shift`+`2` still types a
/// plain `2` there, which is exactly the chord `transform::action_for` gives
/// the increment field.
pub(crate) fn tool_key<'a>(key: &'a str, layout: Layout) -> &'a str {
    if layout != Layout::Azerty {
        return key;
    }
    match key {
        "ampersand" => "1",
        "eacute" => "2",
        "quotedbl" => "3",
        "apostrophe" => "4",
        _ => key,
    }
}

/// Whether a keystroke is a chord — a command modifier is down — rather than
/// a key the camera may read. `Shift` is not one: it is the camera's own
/// precision modifier, and `Shift`+`W` still means forward.
pub(crate) fn chorded(modifiers: Modifiers) -> bool {
    modifiers.control || modifiers.alt || modifiers.platform
}

/// A scroll event in wheel notches, positive away from the user.
pub(crate) fn wheel_notches(delta: ScrollDelta) -> f32 {
    match delta {
        ScrollDelta::Lines(lines) => lines.y / LINES_PER_NOTCH,
        ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / PIXELS_PER_NOTCH,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px};

    use super::*;

    // The bug this whole module exists for: the physical keys W/A/S/D must move
    // the camera the same way whatever letters the keyboard prints on them.
    #[test]
    fn the_wasd_positions_move_the_same_way_on_both_layouts() {
        let positions = [
            (CameraKey::Forward, "w", "z"),
            (CameraKey::Back, "s", "s"),
            (CameraKey::Left, "a", "q"),
            (CameraKey::Right, "d", "d"),
            (CameraKey::Up, "e", "e"),
            (CameraKey::Down, "q", "a"),
        ];

        for (expected, qwerty, azerty) in positions {
            assert_eq!(
                camera_key(qwerty, Layout::Qwerty),
                Some(expected),
                "{qwerty} on QWERTY"
            );
            assert_eq!(
                camera_key(azerty, Layout::Azerty),
                Some(expected),
                "{azerty} on AZERTY"
            );
        }
    }

    #[test]
    fn the_arrows_mirror_the_letters_on_every_layout() {
        for layout in [Layout::Qwerty, Layout::Azerty] {
            assert_eq!(camera_key("up", layout), Some(CameraKey::Forward));
            assert_eq!(camera_key("down", layout), Some(CameraKey::Back));
            assert_eq!(camera_key("left", layout), Some(CameraKey::Left));
            assert_eq!(camera_key("right", layout), Some(CameraKey::Right));
        }
    }

    #[test]
    fn an_unrelated_key_moves_nothing() {
        assert_eq!(camera_key("p", Layout::Qwerty), None);
        assert_eq!(camera_key("enter", Layout::Azerty), None);
        assert_eq!(camera_key("", Layout::Qwerty), None);
    }

    #[test]
    fn french_and_belgian_layouts_are_azerty_but_bepo_is_not() {
        assert_eq!(Layout::of("French"), Layout::Azerty);
        assert_eq!(Layout::of("French (AZERTY)"), Layout::Azerty);
        assert_eq!(Layout::of("Belgian (alt.)"), Layout::Azerty);
        assert_eq!(
            Layout::of("French (Bepo, ergonomic, Dvorak way)"),
            Layout::Qwerty
        );
    }

    #[test]
    fn anything_unrecognized_is_treated_as_qwerty() {
        assert_eq!(Layout::of("English (US)"), Layout::Qwerty);
        assert_eq!(Layout::of("German"), Layout::Qwerty);
        assert_eq!(Layout::of("unknown"), Layout::Qwerty);
    }

    #[test]
    fn azerty_s_unshifted_digit_row_reaches_the_tool_shortcuts() {
        assert_eq!(tool_key("ampersand", Layout::Azerty), "1");
        assert_eq!(tool_key("eacute", Layout::Azerty), "2");
        assert_eq!(tool_key("quotedbl", Layout::Azerty), "3");
        assert_eq!(tool_key("apostrophe", Layout::Azerty), "4");
        // Shifted, the same keys type the digits themselves.
        assert_eq!(tool_key("2", Layout::Azerty), "2");
    }

    #[test]
    fn qwerty_s_punctuation_is_not_a_tool_shortcut() {
        assert_eq!(tool_key("eacute", Layout::Qwerty), "eacute");
        assert_eq!(tool_key("apostrophe", Layout::Qwerty), "apostrophe");
        assert_eq!(tool_key("2", Layout::Qwerty), "2");
    }

    // The bug this exists for: Ctrl+Z with the viewport focused is an undo,
    // and on AZERTY `z` is also the forward key — it must not be both.
    #[test]
    fn a_command_modifier_makes_a_chord_but_shift_does_not() {
        let control = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        let alt = Modifiers {
            alt: true,
            ..Modifiers::none()
        };
        let platform = Modifiers {
            platform: true,
            ..Modifiers::none()
        };
        let shift = Modifiers {
            shift: true,
            ..Modifiers::none()
        };
        assert!(chorded(control));
        assert!(chorded(alt));
        assert!(chorded(platform));
        assert!(!chorded(shift));
        assert!(!chorded(Modifiers::none()));
    }

    #[test]
    fn three_scrolled_lines_are_one_notch() {
        assert_eq!(wheel_notches(ScrollDelta::Lines(point(0.0, 3.0))), 1.0);
        assert_eq!(wheel_notches(ScrollDelta::Lines(point(0.0, -6.0))), -2.0);
    }

    #[test]
    fn a_pixel_delta_is_scaled_down_to_notch_units() {
        let up = wheel_notches(ScrollDelta::Pixels(point(px(0.0), px(PIXELS_PER_NOTCH))));
        assert!((up - 1.0).abs() < 1e-6);

        let down = wheel_notches(ScrollDelta::Pixels(point(
            px(0.0),
            px(-2.0 * PIXELS_PER_NOTCH),
        )));
        assert!((down + 2.0).abs() < 1e-6);
    }

    #[test]
    fn lines_and_pixels_agree_on_direction() {
        let lines = wheel_notches(ScrollDelta::Lines(point(0.0, LINES_PER_NOTCH)));
        let pixels = wheel_notches(ScrollDelta::Pixels(point(px(0.0), px(PIXELS_PER_NOTCH))));

        assert_eq!(lines.signum(), pixels.signum());
    }

    // Horizontal scrolling is not a camera control: a sideways swipe must not
    // creep the camera forward.
    #[test]
    fn a_horizontal_scroll_is_no_notch_at_all() {
        assert_eq!(wheel_notches(ScrollDelta::Lines(point(9.0, 0.0))), 0.0);
    }
}
