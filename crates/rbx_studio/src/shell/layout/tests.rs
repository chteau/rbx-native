use super::*;

/// One dock as a settings file would hold it, showing its first tab.
fn group(panels: &[&str]) -> SavedGroup {
    SavedGroup {
        panels: panels.iter().map(|name| (*name).to_owned()).collect(),
        active: 0,
    }
}

/// One edge holding one dock per named run of panels.
fn edge(edge: Edge, groups: &[&[&str]], size: f32) -> SavedEdge {
    SavedEdge {
        edge,
        groups: groups.iter().map(|panels| group(panels)).collect(),
        size,
    }
}

fn saved(edges: Vec<SavedEdge>) -> SavedLayout {
    SavedLayout {
        edges,
        floating: Vec::new(),
        closed: Vec::new(),
    }
}

/// What one edge holds, as panel lists — the shape most of these assert on.
fn shape(layout: &Layout, edge: Edge) -> Vec<Vec<Panel>> {
    layout
        .groups(edge)
        .iter()
        .map(|group| group.panels().to_vec())
        .collect()
}

/// The layout nobody has touched is the one the editor always had:
/// Properties on the left, Explorer on the right, Output underneath — with
/// the Viewport dock a tab behind Output rather than a dock of its own.
#[test]
fn the_default_layout_is_the_shell_that_was_hardcoded() {
    let layout = Layout::default();

    assert_eq!(shape(&layout, Edge::Left), [[Panel::Properties]]);
    assert_eq!(shape(&layout, Edge::Right), [[Panel::Explorer]]);
    assert_eq!(
        shape(&layout, Edge::Bottom),
        [[Panel::Output, Panel::Viewport]]
    );
    assert!(layout.floating().is_empty());
}

/// Behind Output, not in front of it: the Viewport dock samples the frame
/// rate only while it is on screen, and a fresh editor should not.
#[test]
fn the_viewport_dock_starts_open_but_not_showing() {
    let layout = Layout::default();

    assert_ne!(layout.home_of(Panel::Viewport), Home::Closed);
    assert!(!layout.is_showing(Panel::Viewport));
    assert!(layout.is_showing(Panel::Output));
}

/// What is on screen is exactly the tabs being shown and the windows of
/// their own — never a tab behind another, never a shut panel.
#[test]
fn showing_follows_the_tab_the_window_and_the_close() {
    let mut layout = Layout::default();

    layout.activate(Panel::Viewport);
    assert!(layout.is_showing(Panel::Viewport));
    assert!(!layout.is_showing(Panel::Output));

    layout.float(Panel::Viewport);
    assert!(layout.is_showing(Panel::Viewport));
    assert!(layout.is_showing(Panel::Output));

    layout.close(Panel::Viewport);
    assert!(!layout.is_showing(Panel::Viewport));
}

/// Opening a panel that is already open but hidden behind another tab
/// brings it forward rather than doing nothing — otherwise the View menu
/// would have no way to show the Viewport dock in a fresh layout.
#[test]
fn opening_a_hidden_tab_brings_it_forward() {
    let mut layout = Layout::default();
    layout.open(Panel::Viewport);

    assert!(layout.is_showing(Panel::Viewport));
    assert_eq!(
        shape(&layout, Edge::Bottom),
        [[Panel::Output, Panel::Viewport]]
    );
}

/// Closing a torn-out dock's window asks for it back, and it comes back
/// docked where it started, showing — not left floating with no window.
#[test]
fn opening_a_floating_panel_docks_it_back_home() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);
    layout.open(Panel::Explorer);

    assert_eq!(layout.floating(), []);
    assert_eq!(shape(&layout, Edge::Right), [[Panel::Explorer]]);
    assert!(layout.is_showing(Panel::Explorer));
}

/// Closed and reopened, it comes back where it started — a tab beside
/// Output — and in front, since reopening is asking to see it.
#[test]
fn a_reopened_viewport_dock_is_a_showing_tab_beside_output() {
    let mut layout = Layout::default();
    layout.close(Panel::Viewport);
    assert_eq!(shape(&layout, Edge::Bottom), [[Panel::Output]]);

    layout.open(Panel::Viewport);

    assert_eq!(
        shape(&layout, Edge::Bottom),
        [[Panel::Output, Panel::Viewport]]
    );
    assert!(layout.is_showing(Panel::Viewport));
}

/// A settings file from before the Viewport dock existed gets it as a
/// hidden tab beside Output — the same place a fresh layout puts it —
/// rather than a second dock halving Output's width.
#[test]
fn a_file_from_before_the_viewport_dock_seats_it_beside_output() {
    let file = saved(vec![
        edge(Edge::Left, &[&["Properties"]], 300.),
        edge(Edge::Right, &[&["Explorer"]], 300.),
        edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
    ]);

    let layout = Layout::restore(&file);

    assert_eq!(
        shape(&layout, Edge::Bottom),
        [[Panel::Output, Panel::Viewport]]
    );
    assert!(!layout.is_showing(Panel::Viewport));
}

/// Where the Viewport dock was, and whether it was shut, persist like any
/// other dock's.
#[test]
fn the_viewport_dock_survives_a_round_trip_moved_or_closed() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Viewport,
        Landing::NewGroup {
            edge: Edge::Right,
            group: 1,
        },
    );
    assert_eq!(Layout::restore(&layout.saved()), layout);

    layout.close(Panel::Viewport);
    let restored = Layout::restore(&layout.saved());
    assert_eq!(restored.home_of(Panel::Viewport), Home::Closed);
    assert_eq!(restored, layout);
}

/// The invariant every method here leans on: a panel is in exactly one
/// place, whatever has been done to it.
#[test]
fn a_panel_is_in_exactly_one_place() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 0,
        },
    );
    layout.float(Panel::Output);
    layout.close(Panel::Explorer);
    layout.apply(
        Panel::Output,
        Landing::NewGroup {
            edge: Edge::Bottom,
            group: 0,
        },
    );

    for panel in Panel::ALL {
        let docked = Edge::ALL
            .into_iter()
            .flat_map(|edge| layout.groups(edge))
            .filter(|group| group.panels().contains(&panel))
            .count();
        let elsewhere = usize::from(layout.floating().contains(&panel))
            + usize::from(layout.home_of(panel) == Home::Closed);
        assert_eq!(docked + elsewhere, 1, "{panel} is in {docked} docks");
    }
}

/// A dock never outlives its last tab — an empty one would render as a
/// strip with nothing under it and take room from the document.
#[test]
fn a_dock_that_loses_its_last_tab_is_gone() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 0,
        },
    );

    assert!(layout.groups(Edge::Left).is_empty());
    assert_eq!(
        shape(&layout, Edge::Right),
        [[Panel::Properties, Panel::Explorer]]
    );
}

/// Landing on a dock's strip makes the panel a tab of it, at the position
/// aimed at.
#[test]
fn landing_on_a_strip_inserts_a_tab_there() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 1,
        },
    );

    assert_eq!(
        shape(&layout, Edge::Right),
        [[Panel::Explorer, Panel::Properties]]
    );
    assert_eq!(
        layout.groups(Edge::Right)[0].active(),
        Some(Panel::Properties),
        "the tab you just dropped is the one you want to look at"
    );
}

/// Landing on a dock's half makes a *new* dock beside it rather than a
/// tab — the distinction the whole overlay exists to offer.
#[test]
fn landing_on_a_half_stacks_a_new_dock() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::NewGroup {
            edge: Edge::Right,
            group: 1,
        },
    );

    assert_eq!(
        shape(&layout, Edge::Right),
        [vec![Panel::Explorer], vec![Panel::Properties]]
    );
}

/// And it can land above the dock it was aimed at, not only below.
#[test]
fn a_new_dock_can_stack_before_the_one_aimed_at() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::NewGroup {
            edge: Edge::Right,
            group: 0,
        },
    );

    assert_eq!(
        shape(&layout, Edge::Right),
        [vec![Panel::Properties], vec![Panel::Explorer]]
    );
}

/// A panel dropped onto its own dock's strip is a reorder, and must not
/// leave a hole where its dock used to be.
#[test]
fn a_panel_dropped_on_its_own_dock_stays_put() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Explorer,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 0,
        },
    );

    assert_eq!(shape(&layout, Edge::Right), [[Panel::Explorer]]);
}

/// The index a landing carries was computed before the panel moved. When
/// taking it out empties a dock *above* its target on the same edge, the
/// target has shifted up by one and the drop has to follow it.
#[test]
fn a_landing_below_the_dock_it_emptied_still_lands_right() {
    let mut layout = Layout::default();
    // Right: [Explorer], [Properties] — Properties alone in dock 1.
    layout.apply(
        Panel::Properties,
        Landing::NewGroup {
            edge: Edge::Right,
            group: 1,
        },
    );
    // Now send Explorer (dock 0) below Properties (dock 1).
    layout.apply(
        Panel::Explorer,
        Landing::NewGroup {
            edge: Edge::Right,
            group: 2,
        },
    );

    assert_eq!(
        shape(&layout, Edge::Right),
        [vec![Panel::Properties], vec![Panel::Explorer]]
    );
}

/// An edge nobody put anything on holds nothing, which is what lets the
/// document take its room.
#[test]
fn an_emptied_edge_holds_nothing() {
    let mut layout = Layout::default();
    layout.close(Panel::Viewport);
    layout.apply(
        Panel::Output,
        Landing::NewGroup {
            edge: Edge::Left,
            group: 0,
        },
    );

    assert!(layout.groups(Edge::Bottom).is_empty());
}

/// Tearing a panel out takes it off its edge.
#[test]
fn floating_a_panel_takes_it_off_its_edge() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);

    assert_eq!(layout.home_of(Panel::Explorer), Home::Floating);
    assert!(layout.groups(Edge::Right).is_empty());
    assert_eq!(layout.floating(), [Panel::Explorer]);
}

/// And dropping it back on an edge takes it out of its window again.
#[test]
fn docking_a_floating_panel_closes_its_window() {
    let mut layout = Layout::default();
    layout.float(Panel::Explorer);
    layout.apply(
        Panel::Explorer,
        Landing::NewGroup {
            edge: Edge::Left,
            group: 0,
        },
    );

    assert_eq!(
        layout.home_of(Panel::Explorer),
        Home::Docked {
            edge: Edge::Left,
            group: 0
        }
    );
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

/// A closed panel is nowhere, and says so — which is what a View menu tick
/// and a ribbon button read.
#[test]
fn a_closed_panel_is_open_nowhere() {
    let mut layout = Layout::default();
    layout.close(Panel::Output);

    assert_eq!(layout.home_of(Panel::Output), Home::Closed);
    assert_eq!(shape(&layout, Edge::Bottom), [[Panel::Viewport]]);
}

/// Reopening puts it back on its own edge rather than nowhere in
/// particular.
#[test]
fn reopening_a_panel_puts_it_on_its_own_edge() {
    let mut layout = Layout::default();
    layout.close(Panel::Output);
    layout.open(Panel::Output);

    assert!(layout.is_showing(Panel::Output));
    assert_eq!(
        shape(&layout, Edge::Bottom),
        [[Panel::Viewport, Panel::Output]]
    );
}

/// Closing a tab must not take the dock's other tabs with it.
#[test]
fn closing_one_tab_leaves_the_rest_of_its_dock() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 1,
        },
    );
    layout.close(Panel::Explorer);

    assert_eq!(shape(&layout, Edge::Right), [[Panel::Properties]]);
    assert_eq!(layout.home_of(Panel::Explorer), Home::Closed);
}

/// Clicking a tab shows it; it does not move anything.
#[test]
fn activating_a_tab_only_changes_what_is_showing() {
    let mut layout = Layout::default();
    layout.apply(
        Panel::Properties,
        Landing::Tab {
            edge: Edge::Right,
            group: 0,
            tab: 1,
        },
    );
    layout.activate(Panel::Explorer);

    assert_eq!(
        layout.groups(Edge::Right)[0].active(),
        Some(Panel::Explorer)
    );
    assert_eq!(
        shape(&layout, Edge::Right),
        [[Panel::Explorer, Panel::Properties]]
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
    layout.apply(
        Panel::Properties,
        Landing::NewGroup {
            edge: Edge::Bottom,
            group: 0,
        },
    );
    layout.float(Panel::Explorer);
    layout.resize(Edge::Right, 420.);

    assert_eq!(Layout::restore(&layout.saved()), layout);
}

/// Including which panels were shut — a closed panel that came back on
/// every restart would be a panel you cannot close.
#[test]
fn a_closed_panel_stays_closed_across_a_round_trip() {
    let mut layout = Layout::default();
    layout.close(Panel::Output);

    let restored = Layout::restore(&layout.saved());

    assert_eq!(restored.home_of(Panel::Output), Home::Closed);
    assert_eq!(restored, layout);
}

/// A settings file from a version that knew a panel this one doesn't must
/// open the editor, not stop it.
#[test]
fn an_unknown_panel_name_is_dropped_rather_than_refused() {
    let file = saved(vec![
        edge(Edge::Left, &[&["Properties", "Terrain Editor"]], 300.),
        edge(Edge::Right, &[&["Explorer"]], 300.),
        edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}

/// And one that has lost a panel altogether gets it back where it started,
/// rather than the editor rendering without it.
#[test]
fn a_panel_missing_from_the_file_comes_back_on_its_own_edge() {
    let file = saved(vec![
        edge(Edge::Left, &[], 300.),
        edge(Edge::Right, &[&["Explorer"]], 300.),
        edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}

/// A dock the file left empty is not a dock.
#[test]
fn an_empty_dock_in_the_file_is_dropped() {
    let file = saved(vec![
        edge(Edge::Left, &[&["Properties"], &[]], 300.),
        edge(Edge::Right, &[&["Explorer"]], 300.),
        edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}

/// A file naming one panel twice would otherwise put it in two places at
/// once, breaking the invariant every method here assumes.
#[test]
fn a_panel_named_twice_still_lands_in_one_place() {
    let file = SavedLayout {
        edges: vec![
            edge(Edge::Left, &[&["Explorer", "Explorer"]], 300.),
            edge(Edge::Right, &[&["Explorer"]], 300.),
            edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
        ],
        floating: vec!["Explorer".to_owned()],
        closed: Vec::new(),
    };

    let layout = Layout::restore(&file);

    assert_eq!(
        layout.home_of(Panel::Explorer),
        Home::Docked {
            edge: Edge::Left,
            group: 0
        }
    );
    assert!(layout.floating().is_empty());
    // Properties, which the file never placed, is seated as a tab of the
    // dock already on its edge rather than splitting it.
    assert_eq!(
        shape(&layout, Edge::Left),
        [[Panel::Explorer, Panel::Properties]]
    );
}

/// A tab index a shortened or unknown-panel list leaves dangling must not
/// leave the dock showing nothing.
#[test]
fn an_active_index_past_the_end_walks_back_to_a_real_tab() {
    let file = saved(vec![
        SavedEdge {
            edge: Edge::Left,
            groups: vec![SavedGroup {
                panels: vec!["Properties".to_owned(), "Terrain Editor".to_owned()],
                active: 1,
            }],
            size: 300.,
        },
        edge(Edge::Right, &[&["Explorer"]], 300.),
        edge(Edge::Bottom, &[&["Output"]], Edge::Bottom.default_size()),
    ]);

    assert_eq!(
        Layout::restore(&file).groups(Edge::Left)[0].active(),
        Some(Panel::Properties)
    );
}

/// A saved size of zero is what `Settings` writes for "never set", and has
/// to mean the default rather than a dock collapsed to nothing.
#[test]
fn an_unset_size_falls_back_to_the_default() {
    let file = saved(vec![
        edge(Edge::Left, &[&["Properties"]], 0.),
        edge(Edge::Right, &[&["Explorer"]], 0.),
        edge(Edge::Bottom, &[&["Output"]], 0.),
    ]);

    assert_eq!(Layout::restore(&file), Layout::default());
}
