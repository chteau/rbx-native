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
    assert!(!g.connect(clip, out, "Nope"), "fixed pins do not grow");
    assert!(g.remove(speed));
    assert_eq!(g.nodes[out].input("Pose"), None);
    assert!(!g.remove(out));
}

#[test]
fn nodes_are_named_like_the_editor_names_them() {
    let mut g = Graph::default();
    assert_eq!(g.nodes[0].id, "GraphOutput");
    let a = g.add(NodeKind::Clip);
    let b = g.add(NodeKind::Clip);
    assert_eq!(
        (g.nodes[a].id.as_str(), g.nodes[b].id.as_str()),
        ("ClipNode", "ClipNode1")
    );
    assert_eq!(g.nodes[a].name, "Clip");
    assert_eq!(NodeKind::Blend1D.label(), "Blend1D");
    assert_eq!(NodeKind::GraphOutput.label(), "Graph Output");
}

#[test]
fn dynamic_pins_are_named_after_their_source() {
    let mut g = Graph::default();
    let a = g.add(NodeKind::Clip);
    let b = g.add(NodeKind::Clip);
    let seq = g.add(NodeKind::Sequence);
    assert!(g.nodes[seq].inputs.is_empty());
    assert_eq!(g.connect_new(a, seq).as_deref(), Some("Clip"));
    assert_eq!(g.connect_new(b, seq).as_deref(), Some("Clip1"));
    g.nodes[a].name = "Idle".into();
    assert_eq!(g.connect_new(a, seq).as_deref(), Some("Idle"));
    assert!(g.nodes[seq].inputs[0].props.contains_key("WaitForTrigger"));
    let speed = g.add(NodeKind::Speed);
    assert_eq!(g.connect_new(a, speed), None, "fixed pins only");
}

#[test]
fn new_pins_follow_the_placeholder_names() {
    let mut g = Graph::default();
    let sm = g.add(NodeKind::StateMachine);
    assert_eq!(g.nodes[sm].add_pin(), Some("New State"));
    assert_eq!(g.nodes[sm].add_pin(), Some("New State1"));
    let blend = g.add(NodeKind::Blend1D);
    let pins: Vec<_> = g.nodes[blend]
        .inputs
        .iter()
        .map(|i| i.pin.as_str())
        .collect();
    assert_eq!(pins, ["A", "B"]);
    assert_eq!(
        g.nodes[blend].inputs[1].props["Position"],
        Prop::Value(Value::Number(1.))
    );
    assert_eq!(g.nodes[blend].add_pin(), Some("Input2"));
    let speed = g.add(NodeKind::Speed);
    assert_eq!(g.nodes[speed].add_pin(), None);
}

#[test]
fn removing_a_state_drops_its_transitions() {
    let mut g = Graph::default();
    let sm = g.add(NodeKind::StateMachine);
    for _ in 0..3 {
        g.nodes[sm].add_pin();
    }
    g.nodes[sm].transitions = vec![
        Transition::new(Some(0), 1),
        Transition::new(Some(2), 0),
        Transition::new(None, 2),
    ];
    assert!(g.nodes[sm].remove_pin("New State1"));
    let ends: Vec<_> = g.nodes[sm]
        .transitions
        .iter()
        .map(|t| (t.from, t.to))
        .collect();
    assert_eq!(ends, [(Some(1), 0), (None, 1)]);
}

#[test]
fn new_transitions_and_params_take_the_editor_defaults() {
    let t = Transition::new(None, 0);
    assert_eq!((t.duration, t.priority, t.when), (0.2, 1, When::Finished));
    assert_eq!(t.wait_for, WaitFor::Trigger);
    assert!(t.expression.is_empty());
    let mut g = Graph::default();
    assert_eq!(g.add_param(ParamKind::Number), "NumberParameter");
    assert_eq!(g.add_param(ParamKind::Number), "NumberParameter1");
    assert_eq!(g.add_param(ParamKind::Bool), "BooleanParameter");
    assert_eq!(
        g.param("BooleanParameter").unwrap().default,
        Value::Bool(false)
    );
    assert_eq!(sanitize_name(" RBX_my speed "), "my_speed");
}

#[test]
fn values_coerce_like_luau() {
    assert!(Value::Number(0.).truthy());
    assert!(!Value::Bool(false).truthy());
    assert_eq!(Value::Text(" 2.5".into()).number(), 2.5);
    assert_eq!(Value::Bool(true).number(), 1.);
}

#[test]
fn editor_conveniences_match_the_plugin() {
    let mut g = Graph::default();
    let blend = g.add(NodeKind::Blend1D);
    assert_eq!(g.nodes[blend].placeholder().as_deref(), Some("Input2"));
    let b2 = g.add(NodeKind::Blend2D);
    assert_eq!(g.nodes[b2].placeholder().as_deref(), Some("Input0"));
    let seq = g.add(NodeKind::Sequence);
    assert_eq!(g.nodes[seq].placeholder().as_deref(), Some("New Input"));
    assert_eq!(g.nodes[0].placeholder(), None);
    assert_eq!(
        NodeKind::number_label("LoopCount", 0.),
        Some("0 (Infinity)")
    );
    assert_eq!(NodeKind::number_label("Seed", -1.), Some("-1 (Random)"));
    assert_eq!(NodeKind::number_label("Seed", 3.), None);
    let clip = g.add(NodeKind::Clip);
    g.nodes[clip].assign_clip("rbxassetid://1", 1.5, true);
    assert_eq!(
        g.nodes[clip].props["TrimEnd"],
        Prop::Value(Value::Number(1.5))
    );
}

#[test]
fn new_nodes_take_the_documented_defaults() {
    let prop = |kind: NodeKind, name: &str| {
        kind.default_props()
            .into_iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v)
    };
    let text = |s: &str| Some(Value::Text(s.into()));
    assert_eq!(prop(NodeKind::Clip, "PlayMode"), text("Loop"));
    assert_eq!(prop(NodeKind::Clip, "Speed"), Some(Value::Number(1.)));
    assert_eq!(prop(NodeKind::Over, "Weight"), Some(Value::Number(1.)));
    assert_eq!(prop(NodeKind::Blend1D, "PhaseSync"), text("Synced"));
    assert_eq!(prop(NodeKind::Blend2D, "InputMode"), text("Cartesian"));
    assert_eq!(
        prop(NodeKind::PrioritySelect, "DefaultInterruptible"),
        text("Always")
    );
    assert_eq!(prop(NodeKind::Sequence, "DefaultWaitFor"), text("Finished"));
    assert_eq!(
        prop(NodeKind::Sequence, "LoopCount"),
        Some(Value::Number(0.))
    );
    assert_eq!(
        prop(NodeKind::RandomSequence, "Seed"),
        Some(Value::Number(-1.))
    );
    assert_eq!(
        prop(NodeKind::Select, "DefaultTransitionDuration"),
        Some(Value::Number(DEFAULT_TRANSITION_DURATION))
    );
    assert_eq!(DEFAULT_TRANSITION_DURATION, 0.1);
    assert_eq!(
        prop(NodeKind::OneShot, "Interruptible"),
        Some(Value::Bool(true))
    );
    assert_eq!(
        prop(NodeKind::OneShot, "TransitionOutDuration"),
        Some(Value::Number(0.15))
    );
    assert_eq!(
        prop(NodeKind::OneShot, "TransitionOutWhen"),
        text("Finished")
    );
    let weight = NodeKind::RandomSequence.default_input_props();
    assert_eq!(weight, [("Weight", Value::Number(1.))]);
    assert_eq!(
        ParamKind::PhaseSync.default_value(),
        Value::Text("Synced".into())
    );
    assert_eq!(ParamKind::Trigger.default_value(), Value::Bool(false));
}
