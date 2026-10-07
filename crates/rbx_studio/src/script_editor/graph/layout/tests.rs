use super::super::catalog;
use super::super::{End, Graph};
use super::{chip, chip_text, node_at, pin, pin_at, rect, row_centre, Side, CHIP_CHARS, PIN_REACH};

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
