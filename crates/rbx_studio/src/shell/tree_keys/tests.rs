use super::*;
use std::time::Duration;

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

/// The vendored tree's selection follows its item, not its index (see
/// `vendor/README.md`): opening a row above the selected one used to leave
/// the selection on whatever row slid into its place.
#[gpui_kit::test]
fn the_tree_selection_follows_its_item_across_expansion(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::tree::TreeItem;
    use gpui_kit::AppContext as _;

    let items = vec![
        TreeItem::new("Workspace", "Workspace").child(TreeItem::new("Camera", "Camera")),
        TreeItem::new("Players", "Players")
            .expanded(true)
            .child(TreeItem::new("Player1", "Player1")),
    ];
    let tree = cx.new(|cx| TreeState::new(cx).items(items));
    let selected = |tree: &TreeState| tree.selected_item().map(|item| item.id.to_string());

    tree.update(cx, |tree, cx| {
        tree.set_selected_index(Some(1), cx);
        assert_eq!(selected(tree).as_deref(), Some("Players"));
        // Workspace's chevron, above the selection: it opens and the
        // selection moves down with Players rather than landing on Camera.
        tree.toggle_expanded(0, cx);
        assert_eq!(tree.selected_index(), Some(2));
        assert_eq!(selected(tree).as_deref(), Some("Players"));
        tree.toggle_expanded(0, cx);
        assert_eq!(selected(tree).as_deref(), Some("Players"));
        // Collapsing the selection's own parent hides it: no row selected.
        tree.set_selected_index(Some(2), cx);
        assert_eq!(selected(tree).as_deref(), Some("Player1"));
        tree.toggle_expanded(1, cx);
        assert_eq!(tree.selected_index(), None);
        // Expanding it again brings the selection back, even with another
        // row opened and closed above it in between.
        tree.toggle_expanded(0, cx);
        tree.toggle_expanded(0, cx);
        tree.toggle_expanded(1, cx);
        assert_eq!(selected(tree).as_deref(), Some("Player1"));
        // An explicit deselect while hidden is not undone by expanding.
        tree.toggle_expanded(1, cx);
        tree.set_selected_index(None, cx);
        tree.toggle_expanded(1, cx);
        assert_eq!(tree.selected_index(), None);
    });
}

/// Shift with Up, Down, Home or End is a range move; Ctrl+Shift, a bare
/// arrow or a Shift+Left is not.
#[test]
fn shift_with_a_vertical_move_extends_a_range() {
    let shifted = |name: &str, control: bool| Keystroke {
        modifiers: gpui_kit::Modifiers {
            shift: true,
            control,
            ..Default::default()
        },
        key: name.into(),
        key_char: None,
    };
    assert_eq!(range_nav_for(&shifted("up", false)), Some(Nav::Previous));
    assert_eq!(range_nav_for(&shifted("down", false)), Some(Nav::Next));
    assert_eq!(range_nav_for(&shifted("home", false)), Some(Nav::First));
    assert_eq!(range_nav_for(&shifted("end", false)), Some(Nav::Last));
    assert_eq!(range_nav_for(&shifted("left", false)), None);
    assert_eq!(range_nav_for(&shifted("down", true)), None);
    assert_eq!(range_nav_for(&key("down")), None);
}

/// The APG's alternative multi-select model: Ctrl (or Cmd) with Up/Down
/// moves focus alone and with Space toggles; Shift or Alt with it is
/// something else, and a bare key is the plain contract.
#[test]
fn ctrl_moves_focus_or_toggles_and_nothing_else_does() {
    let with = |name: &str, modifiers: gpui_kit::Modifiers| Keystroke {
        modifiers,
        key: name.into(),
        key_char: None,
    };
    let ctrl = gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    };
    let cmd = gpui_kit::Modifiers {
        platform: true,
        ..Default::default()
    };
    assert_eq!(
        focus_key_for(&with("up", ctrl)),
        Some(FocusKey::Move(Nav::Previous))
    );
    assert_eq!(
        focus_key_for(&with("down", cmd)),
        Some(FocusKey::Move(Nav::Next))
    );
    assert_eq!(focus_key_for(&with("space", ctrl)), Some(FocusKey::Toggle));
    assert_eq!(focus_key_for(&key("down")), None);
    assert_eq!(focus_key_for(&key("space")), None);
    assert_eq!(focus_key_for(&with("left", ctrl)), None);
    let ctrl_shift = gpui_kit::Modifiers {
        shift: true,
        ..ctrl
    };
    assert_eq!(focus_key_for(&with("down", ctrl_shift)), None);
}

/// Focus and selection as two pieces of state, driven the way the Explorer
/// drives them: a plain move selects where it lands, Ctrl+Down walks the
/// cursor past a two-row selection without touching it, Ctrl+Space toggles
/// the row under the cursor in, and a collapse above keeps the cursor on its
/// item.
#[gpui_kit::test]
fn the_cursor_moves_apart_from_a_multi_selection(cx: &mut gpui_kit::TestAppContext) {
    use super::super::selection::Selection;
    use gpui_kit::component::tree::TreeItem;
    use gpui_kit::AppContext as _;
    use rbx_dom::Ref;

    let [a, b, c, d] = [1, 2, 3, 4].map(Ref::new);
    let row = |r: Ref| TreeItem::new(crate::explorer::item_id(r), r.value().to_string());
    let folder = TreeItem::new("folder", "Folder").child(TreeItem::new("leaf", "Leaf"));
    let tree = cx.new(|cx| TreeState::new(cx).items(vec![folder, row(a), row(b), row(c), row(d)]));
    let mut selection = Selection::default();

    tree.update(cx, |tree, cx| {
        // Plain arrow onto `a`: cursor and selection agree.
        tree.set_selected_index(Some(1), cx);
        assert_eq!(tree.focused_index(), Some(1));
        selection.set(Selection::of_item(tree.selected_item()));
        // Shift+Down extends to `b`: the cursor moves, the anchor stays.
        selection.replace(vec![a, b]);
        tree.set_focused_index(Some(2), cx);
        assert_eq!(tree.selected_index(), Some(1));
        // Ctrl+Down twice: past `b` onto `d`, selection untouched.
        tree.set_focused_index(Some(3), cx);
        tree.set_focused_index(Some(4), cx);
        assert_eq!(tree.focused_index(), Some(4));
        assert_eq!(tree.selected_index(), Some(1));
        assert_eq!(selection.all(), [a, b]);
        // Ctrl+Space on `d` adds it; on `a`, the anchor, drops it.
        selection.toggle(d);
        assert_eq!(selection.all(), [a, b, d]);
        selection.toggle(a);
        assert_eq!(selection.get(), Some(b));
        // Opening the folder above shifts every row; both follow their item.
        tree.toggle_expanded(0, cx);
        assert_eq!(tree.focused_index(), Some(5));
        assert_eq!(tree.selected_index(), Some(2));
        // A cursor collapsed out of sight falls back to the selected row.
        tree.set_focused_index(Some(1), cx);
        tree.toggle_expanded(0, cx);
        assert_eq!(tree.focused_index(), tree.selected_index());
        // Selecting nothing leaves the cursor where it was; new items reset it.
        tree.set_focused_index(Some(3), cx);
        tree.set_selected_index(None, cx);
        assert_eq!(tree.focused_index(), Some(3));
        tree.set_items(vec![row(a)], cx);
        assert_eq!(tree.focused_index(), None);
    });
}

/// A multi-selection reports every one of its rows selected, through the
/// tree's own entry state (what `aria_selected` is set from), across a
/// search's `set_items` too; without the set, the single row does.
#[gpui_kit::test]
fn every_row_of_a_multi_selection_reports_itself_selected(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::tree::TreeItem;
    use gpui_kit::AppContext as _;

    let rows = || {
        ["a", "b", "c", "d"]
            .map(|id| TreeItem::new(id, id))
            .to_vec()
    };
    let tree = cx.new(|cx| TreeState::new(cx).items(rows()));
    tree.update(cx, |tree, cx| {
        tree.set_selected_index(Some(0), cx);
        assert_eq!(
            (0..4).map(|ix| tree.is_selected(ix)).collect::<Vec<_>>(),
            [true, false, false, false]
        );
        tree.set_selected_ids(["a".into(), "c".into()]);
        assert_eq!(
            (0..4).map(|ix| tree.is_selected(ix)).collect::<Vec<_>>(),
            [true, false, true, false]
        );
        tree.set_items(rows(), cx);
        assert!(
            tree.is_selected(2),
            "the caller's selection outlives new rows"
        );
        assert!(!tree.is_selected(9), "no row, not selected");
    });
}
