use super::super::catalog;
use super::super::{End, Graph, Group};
use super::{
    chip, chip_at, chip_rect, chip_text, group_title, group_title_at, node_at, pin, pin_at, rect,
    row_centre, Side, CHIP_CHARS, PIN_REACH,
};

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
