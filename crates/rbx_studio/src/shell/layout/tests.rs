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

/// A whole saved layout with nothing floating.
fn saved(edges: Vec<SavedEdge>) -> SavedLayout {
    SavedLayout {
        edges,
        floating: Vec::new(),
    }
}

/// The layout nobody has touched is the one the editor always had:
/// Properties on the left, Explorer on the right, Output underneath.
#[test]
fn the_default_layout_is_the_shell_that_was_hardcoded() {
    let layout = Layout::default();

    assert_eq!(layout.panels(Edge::Left), [Panel::Properties]);
    assert_eq!(layout.panels(Edge::Right), [Panel::Explorer]);
    assert_eq!(layout.panels(Edge::Bottom), [Panel::Output]);
    assert!(layout.floating().is_empty());
}

/// The invariant every method here leans on: a panel is in exactly one
/// place, before and after any rearrangement.
#[test]
fn a_panel_is_in_exactly_one_place() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);
    layout.float(Panel::Output);
    layout.dock(Panel::Explorer, Edge::Bottom, None);
    layout.dock(Panel::Output, Edge::Bottom, None);

    for panel in Panel::ALL {
        let docked = Edge::ALL
            .into_iter()
            .filter(|edge| layout.panels(*edge).contains(&panel))
            .count();
        let floating = usize::from(layout.floating().contains(&panel));
        assert_eq!(docked + floating, 1, "{panel} is in {docked} docks");
    }
}

/// Docking a panel takes it off the edge it was on — the whole point, and
/// the thing source order could not express.
#[test]
fn docking_a_panel_empties_the_edge_it_left() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);

    assert!(layout.panels(Edge::Left).is_empty());
    assert_eq!(layout.home_of(Panel::Properties), Home::Docked(Edge::Right));
    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Explorer, Panel::Properties]
    );
}

/// Several panels on one edge are tabs of it, and the one you just dropped
/// is the one showing.
#[test]
fn a_dropped_panel_becomes_the_tab_on_show() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);

    assert_eq!(layout.active(Edge::Right), Some(Panel::Properties));
}

/// A drop between two tabs reorders the strip rather than only appending,
/// which is what makes a tab strip draggable within itself.
#[test]
fn dropping_before_a_tab_inserts_at_that_position() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);
    layout.dock(Panel::Output, Edge::Right, Some(0));

    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Output, Panel::Explorer, Panel::Properties]
    );
    assert_eq!(layout.active(Edge::Right), Some(Panel::Output));
}

/// Reordering a tab *within* its own strip is a real move, unlike dropping
/// it back on the edge it already lives on.
#[test]
fn a_tab_can_be_reordered_inside_its_own_strip() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);
    layout.dock(Panel::Properties, Edge::Right, Some(0));

    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Properties, Panel::Explorer]
    );
}

/// Dropping a panel on the edge it already lives on changes nothing —
/// otherwise the menu entry for its own edge would shuffle its tabs.
#[test]
fn docking_a_panel_on_its_own_edge_does_nothing() {
    let mut layout = Layout::default();
    let before = layout.clone();
    layout.dock(Panel::Explorer, Edge::Right, None);

    assert_eq!(layout, before);
}

/// An edge whose showing tab walks away has to fall back to one it still
/// holds; an index past the end renders an empty dock.
#[test]
fn an_edge_keeps_showing_something_when_its_active_tab_leaves() {
    let mut layout = Layout::default();
    layout.dock(Panel::Output, Edge::Left, None);
    layout.dock(Panel::Explorer, Edge::Left, None);
    assert_eq!(layout.active(Edge::Left), Some(Panel::Explorer));

    layout.dock(Panel::Explorer, Edge::Right, None);

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
    layout.dock(Panel::Output, Edge::Left, None);

    assert_eq!(layout.active(Edge::Bottom), None);
    assert!(layout.panels(Edge::Bottom).is_empty());
}

/// Tearing a panel out takes it off its edge.
#[test]
fn floating_a_panel_takes_it_off_its_edge() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);

    assert_eq!(layout.home_of(Panel::Explorer), Home::Floating);
    assert!(layout.panels(Edge::Right).is_empty());
    assert_eq!(layout.floating(), [Panel::Explorer]);
}

/// And dropping it back on an edge takes it out of its window again.
#[test]
fn docking_a_floating_panel_closes_its_window() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);
    layout.dock(Panel::Explorer, Edge::Left, None);

    assert_eq!(layout.home_of(Panel::Explorer), Home::Docked(Edge::Left));
    assert!(layout.floating().is_empty());
}

/// Floating one that is already floating must not list it twice, which
/// would open a second window for the same panel.
#[test]
fn floating_a_floating_panel_does_nothing() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);
    let before = layout.clone();
    layout.float(Panel::Explorer);

    assert_eq!(layout, before);
}

/// Clicking a tab shows it; it does not move anything.
#[test]
fn activating_a_tab_only_changes_what_is_showing() {
    let mut layout = Layout::default();
    layout.dock(Panel::Properties, Edge::Right, None);
    layout.activate(Panel::Explorer);

    assert_eq!(layout.active(Edge::Right), Some(Panel::Explorer));
    assert_eq!(
        layout.panels(Edge::Right),
        [Panel::Explorer, Panel::Properties]
    );
}

/// A floating panel has no tab to activate, and asking must not dock it.
#[test]
fn activating_a_floating_panel_leaves_it_floating() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);
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
    layout.dock(Panel::Properties, Edge::Bottom, None);
    layout.float(Panel::Explorer);
    layout.resize(Edge::Right, 420.);

    assert_eq!(Layout::restore(&layout.saved()), layout);
}

/// A settings file from a version that knew a panel this one doesn't must
/// open the editor, not stop it.
#[test]
fn an_unknown_panel_name_is_dropped_rather_than_refused() {
    let file = saved(vec![
        edge(Edge::Left, &["Properties", "Terrain Editor"], 300.),
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}

/// And one that has lost a panel altogether gets it back where it started,
/// rather than the editor rendering without it.
#[test]
fn a_panel_missing_from_the_file_comes_back_on_its_own_edge() {
    let file = saved(vec![
        edge(Edge::Left, &[], 300.),
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}

/// A file naming one panel twice would otherwise put it in two places at
/// once, breaking the invariant every method here assumes.
#[test]
fn a_panel_named_twice_still_lands_in_one_place() {
    let file = SavedLayout {
        edges: vec![
            edge(Edge::Left, &["Explorer"], 300.),
            edge(Edge::Right, &["Explorer"], 300.),
            edge(Edge::Bottom, &["Output"], 180.),
        ],
        floating: vec!["Explorer".to_owned()],
    };

    let layout = Layout::restore(&file);

    // The first mention wins, and Properties — named nowhere — lands on
    // its own default edge rather than filling a gap a duplicate left.
    assert_eq!(layout.home_of(Panel::Explorer), Home::Docked(Edge::Left));
    assert!(layout.floating().is_empty());
    assert_eq!(
        layout.panels(Edge::Left),
        [Panel::Explorer, Panel::Properties]
    );
}

/// A tab index a shortened or unknown-panel list leaves dangling must not
/// leave the dock showing nothing.
#[test]
fn an_active_index_past_the_end_walks_back_to_a_real_tab() {
    let file = saved(vec![
        SavedEdge {
            edge: Edge::Left,
            panels: vec!["Properties".to_owned(), "Terrain Editor".to_owned()],
            size: 300.,
            active: 1,
        },
        edge(Edge::Right, &["Explorer"], 300.),
        edge(Edge::Bottom, &["Output"], 180.),
    ]);

    assert_eq!(
        Layout::restore(&file).active(Edge::Left),
        Some(Panel::Properties)
    );
}

/// A saved size of zero is what `Settings` writes for "never set", and has
/// to mean the default rather than a dock collapsed to nothing.
#[test]
fn an_unset_size_falls_back_to_the_default() {
    let file = saved(vec![
        edge(Edge::Left, &["Properties"], 0.),
        edge(Edge::Right, &["Explorer"], 0.),
        edge(Edge::Bottom, &["Output"], 0.),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}
