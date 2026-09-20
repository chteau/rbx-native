use super::next_title;

/// Left from the first title lands on the last, not nowhere — the same
/// circular reading `shell::roving` gives the ribbon and the tab strips.
#[test]
fn the_titles_wrap_in_both_directions() {
    assert_eq!(next_title(0, 4, true), 3);
    assert_eq!(next_title(3, 4, false), 0);
    assert_eq!(next_title(1, 4, false), 2);
    assert_eq!(next_title(1, 4, true), 0);
}

/// A bar with one title, or none at all, must not index off the end.
#[test]
fn a_single_title_or_none_has_nowhere_to_go_and_does_not_panic() {
    for back in [true, false] {
        assert_eq!(next_title(0, 1, back), 0);
        assert_eq!(next_title(0, 0, back), 0);
    }
}
