use super::*;

/// Wrapping is what makes a short strip feel circular rather than broken:
/// Left from the first tab lands on the last, not nowhere.
#[test]
fn arrows_wrap_around_a_strip() {
    assert_eq!(Move::Previous.apply(0, 3), 2);
    assert_eq!(Move::Next.apply(2, 3), 0);
    assert_eq!(Move::Next.apply(0, 3), 1);
    assert_eq!(Move::Previous.apply(2, 3), 1);
}

#[test]
fn home_and_end_reach_the_ends_of_any_strip() {
    assert_eq!(Move::First.apply(2, 5), 0);
    assert_eq!(Move::Last.apply(0, 5), 4);
}

/// A ribbon page with a single control, or none at all, must not index off
/// the end — `Avatar` and `Plugins` are exactly this shape today.
#[test]
fn a_one_item_or_empty_group_has_nowhere_to_move_and_does_not_panic() {
    for movement in [Move::Previous, Move::Next, Move::First, Move::Last] {
        assert_eq!(movement.apply(0, 1), 0);
        assert_eq!(movement.apply(0, 0), 0);
    }
}

/// The APG's toolbar pattern reserves the arrows for navigation, but only
/// unmodified: Ctrl+Left is a text-editing gesture and must fall through to
/// whatever owns it.
#[test]
fn a_modified_arrow_is_not_a_navigation_key() {
    let modified = Keystroke {
        modifiers: gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        },
        key: "left".into(),
        key_char: None,
    };
    assert_eq!(Move::of(&modified, false), None);

    let plain = Keystroke {
        modifiers: gpui_kit::Modifiers::default(),
        key: "left".into(),
        key_char: None,
    };
    assert_eq!(Move::of(&plain, false), Some(Move::Previous));
}

/// A horizontal strip must not answer Up/Down — those belong to whatever
/// list is underneath it.
#[test]
fn a_horizontal_group_ignores_the_vertical_arrows() {
    let up = Keystroke {
        modifiers: gpui_kit::Modifiers::default(),
        key: "up".into(),
        key_char: None,
    };
    assert_eq!(Move::of(&up, false), None);
    assert_eq!(Move::of(&up, true), Some(Move::Previous));
}
