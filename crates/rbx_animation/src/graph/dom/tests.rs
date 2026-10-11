use super::*;

fn sample() -> Graph {
    let mut g = Graph {
        name: "Locomotion".into(),
        ..Graph::default()
    };
    g.params = vec![
        Param {
            name: "speed".into(),
            kind: ParamKind::Number,
            default: Value::Number(0.),
        },
        Param {
            name: "jump".into(),
            kind: ParamKind::Trigger,
            default: Value::Bool(false),
        },
        Param {
            name: "mode".into(),
            kind: ParamKind::Text,
            default: Value::Text("walk".into()),
        },
    ];
    let idle = g.add(NodeKind::Clip);
    let run = g.add(NodeKind::Clip);
    g.nodes[run]
        .props
        .insert("Speed".into(), Prop::Param("speed".into()));
    g.nodes[run].position = (120., -40.);
    let sm = g.add(NodeKind::StateMachine);
    g.connect(idle, sm, "State1");
    g.connect(run, sm, "State2");
    g.nodes[sm]
        .props
        .insert("EntryState".into(), Prop::Expr("\"State1\"".into()));
    g.nodes[sm].inputs[1]
        .props
        .insert("Note".into(), Prop::Value(Value::Number(2.)));
    let mut t = Transition::new(Some(0), 1);
    t.expression = "speed > 1".into();
    t.priority = 3;
    t.curve = Style::CubicV2;
    t.duration = 0.3;
    let mut back = Transition::new(None, 0);
    back.wait_for = WaitFor::Finished;
    g.nodes[sm].transitions = vec![t, back];
    let out = g.output().unwrap();
    g.connect(sm, out, "Pose");
    g
}

#[test]
fn round_trips_through_instances() {
    let mut dom = WeakDom::new();
    let graph = sample();
    let def = write(&mut dom, None, &graph);
    let back = read(&dom, def);
    assert_eq!(back.name, graph.name);
    let mut params = back.params.clone();
    params.sort_by(|a, b| a.name.cmp(&b.name));
    let mut expected = graph.params.clone();
    expected.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(params, expected);
    assert_eq!(back.nodes, graph.nodes);
}

#[test]
fn uses_roblox_classes_and_pins() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    assert_eq!(dom.get(def).unwrap().class(), "AnimationGraphDefinition");
    let nodes = children(&dom, def, "AnimationNodeDefinition");
    assert_eq!(nodes.len(), 4);
    let sm = nodes[3];
    assert_eq!(
        dom.get(sm).unwrap().properties()["NodeType"],
        Variant::Enum(16)
    );
    let pins = children(&dom, sm, "ObjectValue");
    assert_eq!(dom.get(pins[0]).unwrap().name(), "State1");
    assert_eq!(reference(&dom, pins[0], "Value"), Some(nodes[1]));
    let transitions = children(&dom, sm, "StateMachineTransitionDefinition");
    assert_eq!(transitions.len(), 2);
    assert_eq!(reference(&dom, transitions[1], "From"), None, "(Any)");
    assert_eq!(
        dom.get(transitions[0]).unwrap().properties()["Priority"],
        Variant::Int32(3)
    );
}

#[test]
fn overwrite_keeps_the_definition() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let smaller = Graph {
        name: "Empty".into(),
        ..Graph::default()
    };
    overwrite(&mut dom, def, &smaller);
    let back = read(&dom, def);
    assert_eq!(back.name, "Empty");
    assert_eq!(back.nodes.len(), 1);
    assert!(back.params.is_empty());
}

#[test]
fn an_empty_definition_still_has_an_output() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "G", None);
    assert!(read(&dom, def).output().is_some());
}
