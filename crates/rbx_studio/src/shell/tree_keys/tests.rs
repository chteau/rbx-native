use super::*;

fn key(name: &str) -> Keystroke {
    Keystroke {
        modifiers: gpui_kit::Modifiers::default(),
        key: name.into(),
        key_char: None,
    }
}

/// The APG's Right Arrow contract, all three branches. Getting the middle
/// one wrong is the common bug: a tree that expands a folder but then won't
/// step into it makes the keyboard feel like it stops working halfway down.
#[test]
fn right_arrow_expands_a_closed_node_then_steps_into_an_open_one() {
    assert_eq!(
        nav_for(&key("right"), true, false),
        Some(Nav::LetToolkitExpandOrCollapse),
        "a closed folder opens, and focus stays put"
    );
    assert_eq!(
        nav_for(&key("right"), true, true),
        Some(Nav::Into),
        "an open folder hands focus to its first child"
    );
    assert_eq!(
        nav_for(&key("right"), false, false),
        Some(Nav::Nowhere),
        "a leaf has nothing to the right — but still swallows the key, so \
         the toolkit's wrapping binding underneath never sees it"
    );
}

/// Left's two branches: close what is open, otherwise climb out. The second
/// is the one the toolkit is missing, and it is what makes Left usable as
/// "back out of this subtree".
#[test]
fn left_arrow_closes_an_open_node_then_climbs_to_the_parent() {
    assert_eq!(
        nav_for(&key("left"), true, true),
        Some(Nav::LetToolkitExpandOrCollapse)
    );
    assert_eq!(nav_for(&key("left"), true, false), Some(Nav::OutToParent));
    assert_eq!(nav_for(&key("left"), false, false), Some(Nav::OutToParent));
}

#[test]
fn home_and_end_reach_the_ends_of_the_visible_tree() {
    assert_eq!(nav_for(&key("home"), false, false), Some(Nav::First));
    assert_eq!(nav_for(&key("end"), false, false), Some(Nav::Last));
}

/// Ctrl+Left belongs to whatever binds it, not to the tree.
#[test]
fn a_modified_arrow_is_left_alone() {
    let modified = Keystroke {
        modifiers: gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        },
        key: "left".into(),
        key_char: None,
    };
    assert_eq!(nav_for(&modified, true, true), None);
}

/// ```text
///   Workspace      depth 0   index 0
///   |- Camera      depth 1   index 1
///   |- Model       depth 1   index 2
///   |  \- Part     depth 2   index 3
///   \- Terrain     depth 1   index 4
/// ```
#[test]
fn a_row_climbs_to_the_nearest_shallower_row_above_it() {
    let depths = [0, 1, 1, 2, 1];

    assert_eq!(parent_of(&depths, 3), Some(2), "Part climbs to Model");
    assert_eq!(parent_of(&depths, 2), Some(0), "Model climbs to Workspace");
    assert_eq!(
        parent_of(&depths, 4),
        Some(0),
        "Terrain climbs to Workspace"
    );
    assert_eq!(parent_of(&depths, 0), None, "a root has nowhere to climb");
}

#[test]
fn type_ahead_jumps_to_the_next_row_starting_with_what_was_typed() {
    let labels = [
        "Workspace".to_string(),
        "Camera".to_string(),
        "Baseplate".to_string(),
        "SpawnLocation".to_string(),
    ];

    assert_eq!(typeahead_target(&labels, 0, "sp"), Some(3));
    assert_eq!(
        typeahead_target(&labels, 0, "ba"),
        Some(2),
        "matching is case-insensitive on both sides"
    );
    assert_eq!(typeahead_target(&labels, 0, "zz"), None);
}

/// One letter pressed repeatedly walks the matches rather than sticking on
/// the first — which is how every file manager behaves, and the only way a
/// single letter is useful in a tree with several `Part`s.
#[test]
fn repeating_one_letter_cycles_through_its_matches() {
    let labels = [
        "Part".to_string(),
        "Platform".to_string(),
        "Wall".to_string(),
        "Post".to_string(),
    ];

    assert_eq!(typeahead_target(&labels, 0, "p"), Some(1));
    assert_eq!(typeahead_target(&labels, 1, "p"), Some(3));
    assert_eq!(
        typeahead_target(&labels, 3, "p"),
        Some(0),
        "and wraps, because a search that stops at the bottom has just failed"
    );
}

/// A multi-character query matches from where focus already is, so typing
/// "pa" doesn't skip the `Part` that is already selected.
#[test]
fn a_longer_query_does_not_skip_the_row_already_focused() {
    let labels = ["Part".to_string(), "Platform".to_string()];
    assert_eq!(typeahead_target(&labels, 0, "pa"), Some(0));
}

#[test]
fn a_stale_buffer_starts_a_fresh_query() {
    let mut typeahead = Typeahead::default();
    let start = Instant::now();

    assert_eq!(typeahead.push('p', start), "p");
    assert_eq!(
        typeahead.push('a', start + Duration::from_millis(200)),
        "pa"
    );
    assert_eq!(
        typeahead.push('w', start + Duration::from_secs(5)),
        "w",
        "a pause longer than the timeout means a new search, not 'paw'"
    );
}

#[test]
fn a_command_keystroke_is_never_type_ahead() {
    let ctrl_c = Keystroke {
        modifiers: gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        },
        key: "c".into(),
        key_char: Some("c".into()),
    };
    assert_eq!(typeahead_char(&ctrl_c), None);
    assert_eq!(typeahead_char(&key("enter")), None);
    assert_eq!(typeahead_char(&key("c")), Some('c'));
}
