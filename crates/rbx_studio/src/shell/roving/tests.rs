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

/// The ribbon's control count now changes with what the editor *holds*, not
/// just with which page is open: a tile that greys because there is nothing
/// to paste leaves the group (see `shell::ribbon`). A group that shrinks
/// past its focused index has to pull that index back, or the next arrow
/// key moves against a strip that no longer has the item it is standing on.
#[test]
fn a_group_that_shrinks_pulls_the_focused_index_back_into_range() {
    let nav = Roving::horizontal();
    nav.cursor.set(5);
    nav.finish();
    nav.current.set(4);

    // The next render builds three: two controls greyed out between frames.
    nav.cursor.set(3);
    nav.finish();

    assert_eq!(nav.len.get(), 3);
    assert_eq!(nav.current.get(), 2, "the last item of the smaller group");
    assert_eq!(
        Move::Next.apply(nav.current.get(), nav.len.get()),
        0,
        "and an arrow from there still wraps inside the group"
    );
}

/// The degenerate end of the same rule: a group with nothing left in it.
#[test]
fn a_group_that_empties_leaves_the_focused_index_at_zero() {
    let nav = Roving::horizontal();
    nav.cursor.set(2);
    nav.finish();
    nav.current.set(1);

    nav.cursor.set(0);
    nav.finish();

    assert_eq!(nav.len.get(), 0);
    assert_eq!(nav.current.get(), 0);
}

/// The group's keys are its own only while one of its items holds focus.
/// Its listener sits on a container that holds other controls too — the
/// Viewport dock's quality select beside its settings — and a Home pressed
/// in that open dropdown must stay there rather than jump into the list.
#[gpui_kit::test]
fn keys_pass_through_while_focus_is_outside_the_group(cx: &mut gpui_kit::TestAppContext) {
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let nav = Roving::vertical();
        nav.begin(&TabOrder::default(), Some(3), cx);
        let home = Keystroke::parse("home").unwrap();

        let outside = cx.focus_handle();
        outside.focus(window, cx);
        assert!(!nav.key(&home, window, cx), "took a key it does not own");
        assert!(outside.is_focused(window));

        nav.handle(2, cx).focus(window, cx);
        assert!(nav.key(&home, window, cx));
        assert!(nav.handle(0, cx).is_focused(window));
    });
}
