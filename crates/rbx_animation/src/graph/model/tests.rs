use super::*;

#[test]
fn node_type_values_match_the_enum() {
    assert_eq!(NodeKind::StateMachine.value(), 16);
    assert_eq!(NodeKind::from_value(15), None);
    for kind in NodeKind::INSERTABLE {
        assert_eq!(NodeKind::from_value(kind.value()), Some(kind));
    }
}

#[test]
fn connect_refuses_cycles_and_remove_unwires() {
    let mut g = Graph::default();
    let speed = g.add(NodeKind::Speed);
    let clip = g.add(NodeKind::Clip);
    let out = g.output().unwrap();
    assert!(g.connect(speed, out, "Pose"));
    assert!(g.connect(clip, speed, "Pose"));
    assert!(!g.connect(out, speed, "Pose"), "would loop");
    assert!(!g.connect(speed, speed, "Pose"));
    assert!(!g.connect(clip, out, "Nope"));
    assert!(g.remove(speed));
    assert_eq!(g.nodes[out].input("Pose"), None);
    assert!(!g.remove(out));
}

#[test]
fn pins_grow_with_numbered_names_and_ids_are_unique() {
    let mut g = Graph::default();
    let sm = g.add(NodeKind::StateMachine);
    assert_eq!(g.nodes[sm].add_pin(), Some("State3"));
    let speed = g.add(NodeKind::Speed);
    assert_eq!(g.nodes[speed].add_pin(), None);
    let other = g.add(NodeKind::StateMachine);
    assert_ne!(g.nodes[sm].id, g.nodes[other].id);
    let blend = g.add(NodeKind::Blend1D);
    assert!(g.nodes[blend].inputs[0].props.contains_key("Position"));
}

#[test]
fn values_coerce_like_luau() {
    assert!(Value::Number(0.).truthy());
    assert!(!Value::Bool(false).truthy());
    assert_eq!(Value::Text(" 2.5".into()).number(), 2.5);
    assert_eq!(Value::Bool(true).number(), 1.);
}
