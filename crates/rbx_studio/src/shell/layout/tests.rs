use super::*;

/// One edge as a settings file would hold it, showing its first tab.
fn edge(edge: Edge, panels: &[&str], size: f32) -> SavedEdge {
    SavedEdge {
        edge,
        panels: panels.iter().map(|name| (*name).to_owned()).collect(),
        size,
        active: 0,
    }
}

/// A tab index a shortened or unknown-panel list leaves dangling must not
/// leave the dock showing nothing.
#[test]
fn an_active_index_past_the_end_walks_back_to_a_real_tab() {
    let saved = vec![
        SavedEdge {
            edge: Edge::Left,
            panels: vec!["Properties".to_owned(), "Terrain Editor".to_owned()],
            size: 300.,
            active: 1,
        },
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ];

    assert_eq!(
        Layout::restore(&saved).active(Edge::Left),
        Some(Panel::Properties)
    );
}

/// The layout nobody has touched is the one the editor always had:
/// Properties on the left, Explorer on the right, Output underneath.
#[test]
fn the_default_layout_is_the_shell_that_was_hardcoded() {
    let layout = Layout::default();

    assert_eq!(layout.panels(Edge::Left), [Panel::Properties]);
    assert_eq!(layout.panels(Edge::Right), [Panel::Explorer]);
    assert_eq!(layout.panels(Edge::Bottom), [Panel::Output]);
}

/// The invariant every method here leans on: a panel is on exactly one
/// edge, before and after any move.
#[test]
fn a_panel_is_on_exactly_one_edge() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Properties, Edge::Right);
    layout.move_panel(Panel::Output, Edge::Right);
    layout.move_panel(Panel::Explorer, Edge::Bottom);

    for panel in Panel::ALL {
        let homes = Edge::ALL
            .into_iter()
            .filter(|edge| layout.panels(*edge).contains(&panel))
            .count();
        assert_eq!(homes, 1, "{panel} is on {homes} edges");
    }
}

/// Moving a panel takes it off the edge it was on — the whole point, and
/// the thing source order could not express.
#[test]
fn moving_a_panel_empties_the_edge_it_left() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Properties, Edge::Right);

    assert!(layout.panels(Edge::Left).is_empty());
    assert_eq!(layout.edge_of(Panel::Properties), Edge::Right);
    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Explorer, Panel::Properties]
    );
}

/// The panel you just moved is the one you want to see, not the one that
/// happened to be there already.
#[test]
fn a_moved_panel_becomes_the_tab_on_show() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Properties, Edge::Right);

    assert_eq!(layout.active(Edge::Right), Some(Panel::Properties));
}

/// Moving a panel where it already is changes nothing — otherwise the menu
/// entry for a panel's own edge would shuffle it to the end of its tabs.
#[test]
fn moving_a_panel_to_its_own_edge_does_nothing() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Explorer, Edge::Right);
    layout.move_panel(Panel::Properties, Edge::Left);
    layout.move_panel(Panel::Explorer, Edge::Left);

    let mut same = Layout::default();
    same.move_panel(Panel::Explorer, Edge::Left);
    assert_eq!(layout, same);
}

/// An edge whose showing tab walks away has to fall back to one it still
/// holds; an index past the end renders an empty dock.
#[test]
fn an_edge_keeps_showing_something_when_its_active_tab_leaves() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Output, Edge::Left);
    layout.move_panel(Panel::Explorer, Edge::Left);
    // Explorer arrived last, so it is the one showing.
    assert_eq!(layout.active(Edge::Left), Some(Panel::Explorer));

    layout.move_panel(Panel::Explorer, Edge::Right);

    assert_eq!(
        layout.panels(Edge::Left),
        [Panel::Properties, Panel::Output]
    );
    assert!(layout.active(Edge::Left).is_some());
}

/// An edge nobody put anything on has no tab to show, which is what lets
/// the document take its room.
#[test]
fn an_emptied_edge_shows_nothing() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Output, Edge::Left);

    assert_eq!(layout.active(Edge::Bottom), None);
    assert!(layout.panels(Edge::Bottom).is_empty());
}

/// Clicking a tab shows it; it does not move anything.
#[test]
fn activating_a_tab_only_changes_what_is_showing() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Properties, Edge::Right);
    layout.activate(Panel::Explorer);

    assert_eq!(layout.active(Edge::Right), Some(Panel::Explorer));
    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Explorer, Panel::Properties]
    );
}

/// A panel that is not on the named edge is not dragged onto it by a tab
/// click that missed.
#[test]
fn activating_a_panel_elsewhere_leaves_it_where_it_is() {
    let mut layout = Layout::default();
    let before = layout.clone();
    layout.activate(Panel::Explorer);

    assert_eq!(layout, before);
}

#[test]
fn a_resize_is_clamped_to_the_edges_own_range() {
    let mut layout = Layout::default();

    layout.resize(Edge::Left, 10_000.);
    assert_eq!(layout.size(Edge::Left), Edge::Left.range().1);

    layout.resize(Edge::Bottom, 0.);
    assert_eq!(layout.size(Edge::Bottom), Edge::Bottom.range().0);
}

/// The window cap is a display cap: it must not overwrite the size the
/// user actually chose, or narrowing a window once would lose it.
#[test]
fn capping_an_edge_does_not_record_the_cap() {
    let mut layout = Layout::default();
    layout.resize(Edge::Left, 500.);

    assert_eq!(layout.capped(Edge::Left, 220.), 220.);
    assert_eq!(layout.size(Edge::Left), 500.);
}

#[test]
fn a_layout_survives_a_round_trip_through_the_settings_file() {
    let mut layout = Layout::default();
    layout.move_panel(Panel::Properties, Edge::Bottom);
    layout.resize(Edge::Right, 420.);

    assert_eq!(Layout::restore(&layout.saved()), layout);
}

/// A settings file from a version that knew a panel this one doesn't must
/// open the editor, not stop it.
#[test]
fn an_unknown_panel_name_is_dropped_rather_than_refused() {
    let saved = vec![
        edge(Edge::Left, &["Properties", "Terrain Editor"], 300.),
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ];

    assert_eq!(Layout::restore(&saved), Layout::default());
}

/// And one that has lost a panel altogether gets it back where it started,
/// rather than the editor rendering without it.
#[test]
fn a_panel_missing_from_the_file_comes_back_on_its_own_edge() {
    let saved = vec![
        edge(Edge::Left, &[], 300.),
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ];

    let layout = Layout::restore(&saved);

    assert_eq!(layout.edge_of(Panel::Properties), Edge::Left);
    assert_eq!(layout, Layout::default());
}

/// A file naming one panel twice would otherwise put it on two edges at
/// once, breaking the invariant every method here assumes.
#[test]
fn a_panel_named_twice_still_lands_on_one_edge() {
    let saved = vec![
        edge(Edge::Left, &["Explorer"], 300.),
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ];

    let layout = Layout::restore(&saved);

    // The first mention wins, and Properties — named nowhere — lands on
    // its own default edge rather than filling the gap the duplicate left.
    assert_eq!(layout.edge_of(Panel::Explorer), Edge::Left);
    assert_eq!(
        layout.panels(Edge::Left),
        [Panel::Explorer, Panel::Properties]
    );
    assert!(layout.panels(Edge::Right).is_empty());
}

/// A saved size of zero is what `Settings` writes for "never set", and has
/// to mean the default rather than a dock collapsed to nothing.
#[test]
fn an_unset_size_falls_back_to_the_default() {
    let saved = vec![
        edge(Edge::Left, &["Properties"], 0.),
        edge(Edge::Right, &["Explorer"], 0.),
        edge(Edge::Bottom, &["Output"], 0.),
    ];

    assert_eq!(Layout::restore(&saved), Layout::default());
}
