use std::collections::BTreeSet;

use super::catalog::{self, PinType};
use super::{End, Graph, Group, Refused};

fn add(graph: &mut Graph, key: &str) -> u32 {
    graph.add(catalog::kind(key).unwrap(), [0.0, 0.0])
}

#[test]
fn a_graph_round_trips_through_its_attribute_text() {
    let mut graph = Graph::default();
    let touched = add(&mut graph, "touched");
    let print = add(&mut graph, "print");
    graph
        .connect(End::new(touched, ""), End::new(print, ""))
        .unwrap();
    graph.set_value(&End::new(print, "Value"), "hi".into());
    assert_eq!(Graph::parse(&graph.to_json()), Some(graph));
}

#[test]
fn text_that_is_not_a_graph_does_not_parse() {
    assert_eq!(Graph::parse("not json"), None);
    assert_eq!(Graph::parse("{}"), Some(Graph::default()));
}

#[test]
fn new_nodes_take_fresh_ids() {
    let mut graph = Graph::default();
    let a = add(&mut graph, "print");
    let b = add(&mut graph, "print");
    assert_ne!(a, b);
    graph.remove(&BTreeSet::from([a]));
    assert_ne!(add(&mut graph, "print"), b);
}

#[test]
fn a_value_input_takes_one_wire() {
    let mut graph = Graph::default();
    let a = add(&mut graph, "number");
    let b = add(&mut graph, "number");
    let sum = add(&mut graph, "add");
    graph
        .connect(End::new(a, "Result"), End::new(sum, "A"))
        .unwrap();
    graph
        .connect(End::new(b, "Result"), End::new(sum, "A"))
        .unwrap();
    assert_eq!(graph.wires.len(), 1);
    assert_eq!(graph.wire_into(&End::new(sum, "A")).unwrap().from.node, b);
}

#[test]
fn a_run_output_gives_one_wire_but_a_run_input_takes_many() {
    let mut graph = Graph::default();
    let start = add(&mut graph, "start");
    let other = add(&mut graph, "player_added");
    let first = add(&mut graph, "print");
    let second = add(&mut graph, "print");
    graph
        .connect(End::new(start, ""), End::new(first, ""))
        .unwrap();
    graph
        .connect(End::new(start, ""), End::new(second, ""))
        .unwrap();
    assert_eq!(graph.wires_from(&End::new(start, "")).count(), 1);
    graph
        .connect(End::new(other, ""), End::new(second, ""))
        .unwrap();
    assert_eq!(graph.wires.len(), 2);
}

#[test]
fn mismatched_types_and_self_wires_are_refused() {
    let mut graph = Graph::default();
    let number = add(&mut graph, "number");
    let parent = add(&mut graph, "get_parent");
    assert_eq!(
        graph.connect(End::new(number, "Result"), End::new(parent, "Instance")),
        Err(Refused::Types(PinType::Number, PinType::Instance))
    );
    let sum = add(&mut graph, "add");
    assert_eq!(
        graph.connect(End::new(sum, "Result"), End::new(sum, "A")),
        Err(Refused::SameNode)
    );
    assert_eq!(
        graph.connect(End::new(number, "Nope"), End::new(sum, "A")),
        Err(Refused::NoSuchPin)
    );
}

#[test]
fn a_value_loop_is_refused() {
    let mut graph = Graph::default();
    let a = add(&mut graph, "add");
    let b = add(&mut graph, "add");
    graph
        .connect(End::new(a, "Result"), End::new(b, "A"))
        .unwrap();
    assert_eq!(
        graph.connect(End::new(b, "Result"), End::new(a, "A")),
        Err(Refused::Loop)
    );
}

#[test]
fn removing_a_node_takes_its_wires() {
    let mut graph = Graph::default();
    let a = add(&mut graph, "number");
    let b = add(&mut graph, "add");
    graph
        .connect(End::new(a, "Result"), End::new(b, "A"))
        .unwrap();
    graph.remove(&BTreeSet::from([a]));
    assert!(graph.wires.is_empty());
    assert_eq!(graph.nodes.len(), 1);
}

#[test]
fn an_unwired_input_reads_its_default_until_typed_over() {
    let mut graph = Graph::default();
    let find = add(&mut graph, "find_child_of_class");
    let class = End::new(find, "Class");
    assert_eq!(graph.value(&class).as_deref(), Some("Humanoid"));
    graph.set_value(&class, "Tool".into());
    assert_eq!(graph.value(&class).as_deref(), Some("Tool"));
    assert_eq!(graph.value(&End::new(find, "Parent")), None);
}

#[test]
fn a_group_moves_the_nodes_drawn_wholly_inside_it() {
    let mut graph = Graph::default();
    let inside = graph.add(catalog::kind("print").unwrap(), [40.0, 40.0]);
    let astride = graph.add(catalog::kind("print").unwrap(), [380.0, 40.0]);
    graph.groups.push(Group {
        title: "Group".into(),
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 200.0,
    });
    assert_eq!(graph.nodes_within(0), vec![inside]);
    assert!(!graph.nodes_within(0).contains(&astride));
    assert!(graph.nodes_within(1).is_empty());
}

/// The graph lives in an attribute so it travels with the place: both file
/// formats must hand back exactly the text written.
#[test]
fn a_graph_survives_saving_the_place_in_either_format() {
    use rbx_dom::{Variant, WeakDom};

    use crate::properties::attributes::{attributes, put_attribute};

    let mut graph = Graph::default();
    let start = add(&mut graph, "start");
    let print = add(&mut graph, "print");
    graph
        .connect(End::new(start, ""), End::new(print, ""))
        .unwrap();
    let json = graph.to_json();

    let mut dom = WeakDom::new();
    let script = dom.new_instance("Script", "Script", None);
    put_attribute(
        &mut dom,
        script,
        super::ATTRIBUTE,
        Some(Variant::String(json.clone())),
    )
    .unwrap();

    let binary = rbx_binary::deserialize(&rbx_binary::serialize(&dom).unwrap()).unwrap();
    let xml = rbx_xml::deserialize(&rbx_xml::serialize(&dom).unwrap()).unwrap();
    for reloaded in [binary, xml] {
        let script = reloaded.root_refs()[0];
        let text = match attributes(&reloaded, script).remove(super::ATTRIBUTE) {
            Some(Variant::String(text)) => text,
            other => panic!("no graph attribute after a reload: {other:?}"),
        };
        assert_eq!(Graph::parse(&text), Some(graph.clone()));
    }
}
