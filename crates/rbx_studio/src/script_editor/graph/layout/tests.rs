use super::super::catalog;
use super::super::{End, Graph, Group};
use super::{
    chip, chip_at, chip_rect, chip_text, group_handle_at, group_title, group_title_at, node_at,
    pin, pin_at, rect, resized, row_centre, Handle, Side, CHIP_CHARS, PIN_REACH,
};

fn framed() -> Graph {
    let mut g = Graph::default();
    g.groups.push(Group {
        title: "Box".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 300.0,
    });
    g
}

#[test]
fn handles_are_found_on_corners_and_edges_only() {
    let g = framed();
    let at = |p| group_handle_at(&g, p, 8.0);
    assert_eq!(at([500.0, 400.0]), Some((0, Handle::SouthEast)));
    assert_eq!(at([102.0, 99.0]), Some((0, Handle::NorthWest)));
    assert_eq!(at([498.0, 101.0]), Some((0, Handle::NorthEast)));
    assert_eq!(at([100.0, 399.0]), Some((0, Handle::SouthWest)));
    assert_eq!(at([300.0, 403.0]), Some((0, Handle::South)));
    assert_eq!(at([300.0, 97.0]), Some((0, Handle::North)));
    assert_eq!(at([97.0, 250.0]), Some((0, Handle::West)));
    assert_eq!(at([503.0, 250.0]), Some((0, Handle::East)));
    assert_eq!(at([300.0, 250.0]), None);
    assert_eq!(at([600.0, 250.0]), None);
}

#[test]
fn resizing_from_each_handle_moves_its_edges() {
    let g = &framed().groups[0];
    let size = |h| {
        let r = resized(g, h, [30.0, 20.0]);
        [r.x, r.y, r.w, r.h]
    };
    assert_eq!(size(Handle::SouthEast), [100.0, 100.0, 430.0, 320.0]);
    assert_eq!(size(Handle::East), [100.0, 100.0, 430.0, 300.0]);
    assert_eq!(size(Handle::South), [100.0, 100.0, 400.0, 320.0]);
    assert_eq!(size(Handle::NorthWest), [130.0, 120.0, 370.0, 280.0]);
    assert_eq!(size(Handle::West), [130.0, 100.0, 370.0, 300.0]);
    assert_eq!(size(Handle::North), [100.0, 120.0, 400.0, 280.0]);
    assert_eq!(size(Handle::NorthEast), [100.0, 120.0, 430.0, 280.0]);
    assert_eq!(size(Handle::SouthWest), [130.0, 100.0, 370.0, 320.0]);
}

#[test]
fn a_resize_stops_at_the_minimum_and_keeps_the_far_edge() {
    let g = &framed().groups[0];
    let r = resized(g, Handle::NorthWest, [900.0, 900.0]);
    assert_eq!((r.x + r.w, r.y + r.h), (500.0, 400.0));
    assert_eq!(r.h, 60.0);
    assert!(r.w >= group_title(g).w);
    let r = resized(g, Handle::SouthEast, [-900.0, -900.0]);
    assert_eq!((r.x, r.y, r.h), (100.0, 100.0, 60.0));
    assert!(r.w >= group_title(g).w);
}

#[test]
fn input_n_and_output_n_share_a_row() {
    let mut g = Graph::default();
    let branch = g.add(catalog::kind("branch").unwrap(), [100.0, 50.0]);
    let condition = pin(&g, &End::new(branch, "Condition"), Side::Input).unwrap();
    let no = pin(&g, &End::new(branch, "False"), Side::Output).unwrap();
    assert_eq!(condition[1], no[1]);
    assert_eq!(condition, [100.0, 50.0 + row_centre(1)]);
    let width = rect(&g, g.node(branch).unwrap()).w;
    assert_eq!(no[0], 100.0 + width);
}

#[test]
fn a_node_is_tall_enough_for_its_longest_side() {
    let mut g = Graph::default();
    let set = g.add(catalog::kind("set_property").unwrap(), [0.0, 0.0]);
    let r = rect(&g, g.node(set).unwrap());
    assert!(r.h > row_centre(3));
    assert!(r.w >= 140.0);
}

#[test]
fn a_wired_input_drops_its_chip_and_the_node_narrows() {
    let mut g = Graph::default();
    let number = g.add(catalog::kind("number").unwrap(), [0.0, 0.0]);
    let set = g.add(catalog::kind("set_attribute").unwrap(), [0.0, 0.0]);
    g.set_value(
        &End::new(set, "Value"),
        "a rather long literal value".into(),
    );
    let wide = rect(&g, g.node(set).unwrap()).w;
    let value = *catalog::kind("set_attribute")
        .unwrap()
        .input("Value")
        .unwrap();
    assert!(chip(&g, g.node(set).unwrap(), &value).is_some());
    g.connect(End::new(number, "Result"), End::new(set, "Value"))
        .unwrap();
    assert!(chip(&g, g.node(set).unwrap(), &value).is_none());
    assert!(rect(&g, g.node(set).unwrap()).w < wide);
}

#[test]
fn long_chips_are_cut_with_an_ellipsis() {
    let cut = chip_text("abcdefghijklmnopqrstuvwxyz");
    assert_eq!(cut.chars().count(), CHIP_CHARS);
    assert!(cut.ends_with('…'));
    assert_eq!(chip_text("short"), "short");
}

#[test]
fn a_press_finds_the_topmost_node_and_the_nearest_pin() {
    let mut g = Graph::default();
    let below = g.add(catalog::kind("print").unwrap(), [0.0, 0.0]);
    let above = g.add(catalog::kind("print").unwrap(), [20.0, 10.0]);
    assert_eq!(node_at(&g, [30.0, 20.0]), Some(above));
    assert_eq!(node_at(&g, [5.0, 5.0]), Some(below));
    assert_eq!(node_at(&g, [-50.0, 0.0]), None);
    let value = pin(&g, &End::new(above, "Value"), Side::Input).unwrap();
    let (end, side) = pin_at(&g, [value[0] + 3.0, value[1]], PIN_REACH).unwrap();
    assert_eq!((end, side), (End::new(above, "Value"), Side::Input));
    assert!(pin_at(&g, [value[0] + 40.0, value[1]], PIN_REACH).is_none());
}

#[test]
fn a_chip_is_found_where_it_is_drawn() {
    let mut g = Graph::default();
    let find = g.add(catalog::kind("find_child_of_class").unwrap(), [40.0, 40.0]);
    let class = chip_rect(&g, g.node(find).unwrap(), "Class").unwrap();
    let inside = [class.x + 2.0, class.y + class.h * 0.5];
    assert_eq!(chip_at(&g, inside), Some(End::new(find, "Class")));
    assert!(chip_rect(&g, g.node(find).unwrap(), "Parent").is_none());
    assert_eq!(chip_at(&g, [class.x - 4.0, class.y + 2.0]), None);
}

#[test]
fn a_group_title_is_found_astride_its_top_edge() {
    let mut g = Graph::default();
    g.groups.push(Group {
        title: "Damage on touch".into(),
        x: 0.0,
        y: 100.0,
        w: 600.0,
        h: 300.0,
    });
    let title = group_title(&g.groups[0]);
    assert_eq!(group_title_at(&g, [title.x + 4.0, 100.0]), Some(0));
    assert_eq!(group_title_at(&g, [title.x + title.w + 20.0, 100.0]), None);
    assert_eq!(group_title_at(&g, [title.x + 4.0, 200.0]), None);
}

#[test]
fn an_any_chip_shows_its_literal_as_the_code_reads_it() {
    let mut g = Graph::default();
    let print = g.add(catalog::kind("print").unwrap(), [0.0, 0.0]);
    let value = *catalog::kind("print").unwrap().input("Value").unwrap();
    g.set_value(&End::new(print, "Value"), "hi".into());
    assert_eq!(
        chip(&g, g.node(print).unwrap(), &value).as_deref(),
        Some("\"hi\"")
    );
    g.set_value(&End::new(print, "Value"), "25".into());
    assert_eq!(
        chip(&g, g.node(print).unwrap(), &value).as_deref(),
        Some("25")
    );
}
