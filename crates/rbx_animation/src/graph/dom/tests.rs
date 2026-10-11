use rbx_dom::{Ref, Variant, Vector2Data, WeakDom};

use super::*;
use crate::easing::Style;
use crate::graph::model::{
    Mask, MaskJoint, NodeKind, Param, ParamKind, Prop, Transition, Value, WaitFor, When,
};

mod legacy;

fn sample() -> Graph {
    let mut g = Graph {
        name: "Locomotion".into(),
        ..Graph::default()
    };
    let param = |name: &str, kind, default| Param {
        name: name.into(),
        kind,
        default,
    };
    g.params = vec![
        param("speed", ParamKind::Number, Value::Number(0.)),
        param("jump", ParamKind::Trigger, Value::Bool(false)),
        param("mode", ParamKind::Text, Value::Text("walk".into())),
        param("play", ParamKind::PlayMode, Value::Text("PingPong".into())),
    ];
    g.rbx_attributes
        .insert("RBX_GraphVersion".into(), Value::Number(3.));
    g.masks = vec![Mask {
        joints: vec![MaskJoint {
            name: "UpperTorso".into(),
            weight: 1.,
            children: vec![MaskJoint {
                name: "RightUpperArm".into(),
                weight: 0.5,
                children: Vec::new(),
            }],
        }],
        ..Mask::new("UpperBody")
    }];
    let idle = g.add(NodeKind::Clip);
    let run = g.add(NodeKind::Clip);
    g.nodes[run].name = "Run".into();
    g.nodes[run]
        .props
        .insert("Speed".into(), Prop::Param("speed".into()));
    g.nodes[run].position = (120., -40.);
    g.nodes[run].collapsed = true;
    let sm = g.add(NodeKind::StateMachine);
    assert_eq!(g.connect_new(idle, sm).as_deref(), Some("Clip"));
    assert_eq!(g.connect_new(run, sm).as_deref(), Some("Run"));
    g.nodes[sm].inputs[1].position = Some((40., 60.));
    g.nodes[sm]
        .props
        .insert("EntryState".into(), Prop::Value(Value::Text("Clip".into())));
    let mut t = Transition::new(Some(0), 1);
    t.expression = "speed > 1".into();
    t.priority = 3;
    t.curve = Style::CubicV2;
    t.duration = 0.3;
    let mut back = Transition::new(None, 0);
    back.wait_for = WaitFor::Finished;
    back.when = When::BeforeFinished;
    g.nodes[sm].transitions = vec![t, back];
    let blend = g.add(NodeKind::Blend1D);
    g.connect(idle, blend, "A");
    g.connect(run, blend, "B");
    g.nodes[blend]
        .props
        .insert("Position".into(), Prop::Expr("speed / 10".into()));
    g.nodes[blend].inputs[0]
        .props
        .insert("Position".into(), Prop::Param("speed".into()));
    let mask = g.add(NodeKind::Mask);
    g.nodes[mask]
        .props
        .insert("Mask".into(), Prop::Value(Value::Text("UpperBody".into())));
    g.connect(blend, mask, "Pose");
    let out = g.output().unwrap();
    g.connect(sm, out, "Pose");
    g
}

fn kid(dom: &WeakDom, of: Ref, name: &str) -> Ref {
    dom.get(of)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn attr(dom: &WeakDom, of: Ref, key: &str) -> Option<Variant> {
    attrs(dom, of).get(key).cloned()
}

fn prop(dom: &WeakDom, of: Ref, key: &str) -> Option<Variant> {
    dom.get(of).unwrap().properties().get(key).cloned()
}

fn text(s: &str) -> Option<Variant> {
    Some(Variant::String(s.into()))
}

#[test]
fn round_trips_through_instances_and_the_binary_format() {
    let mut dom = WeakDom::new();
    let graph = sample();
    let def = write(&mut dom, None, &graph);
    let mut back = read(&dom, def);
    for t in back.nodes.iter_mut().flat_map(|n| &mut n.transitions) {
        assert!(!t.id.is_empty(), "a saved transition has its GUID");
        t.id.clear();
    }
    assert_eq!(back.name, graph.name);
    assert_eq!(back.params, graph.params);
    assert_eq!(back.nodes, graph.nodes);
    assert_eq!(back.masks, graph.masks);
    assert_eq!(back.rbx_attributes, graph.rbx_attributes);
    assert_eq!(back.expressions.len(), 1);
    assert_eq!(back.expressions[0].name, "Expression");
    assert_eq!(back.param_refs.len(), graph.params.len());

    let first = read(&dom, def);
    let bytes = rbx_binary::serialize(&dom).unwrap();
    let loaded = rbx_binary::deserialize(&bytes).unwrap();
    assert_eq!(read(&loaded, loaded.root_refs()[0]), first);
    // Saving what was read changes nothing, ids included.
    let mut again = WeakDom::new();
    let def = write(&mut again, None, &first);
    assert_eq!(read(&again, def), first);
}

#[test]
fn the_definition_holds_parameter_defaults_and_rbx_attributes() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    assert_eq!(dom.get(def).unwrap().class(), "AnimationGraphDefinition");
    assert_eq!(attr(&dom, def, "speed"), Some(Variant::Float64(0.)));
    assert_eq!(attr(&dom, def, "jump"), Some(Variant::Bool(false)));
    assert_eq!(
        attr(&dom, def, "play"),
        Some(Variant::Enum(1)),
        "an EnumItem"
    );
    assert_eq!(
        attr(&dom, def, "RBX_GraphVersion"),
        Some(Variant::Float64(3.))
    );
    assert!(dom.get(def).unwrap().tags().is_empty());

    let jump = kid(&dom, def, "Parameter_jump");
    assert_eq!(dom.get(jump).unwrap().class(), "Folder");
    assert_eq!(attr(&dom, jump, "AnimGraph_ParameterType"), text("Trigger"));
    assert_eq!(attr(&dom, jump, "AnimGraph_BindingName"), text("jump"));
    let speed = kid(&dom, def, "Parameter_speed");
    assert_eq!(attr(&dom, speed, "AnimGraph_ParameterType"), text("Number"));
    let run = kid(&dom, def, "ClipNode1");
    let hint = kid(&dom, speed, "connectionHint_Speed");
    assert_eq!(reference(&dom, hint, "Value"), Some(run));
    let play = kid(&dom, def, "Parameter_play");
    assert_eq!(
        attr(&dom, play, "AnimGraph_ParameterType"),
        text("PlayMode")
    );
}

#[test]
fn nodes_keep_their_properties_as_attributes_and_layout_in_a_configuration() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let names: Vec<String> = children(&dom, def, "AnimationNodeDefinition")
        .into_iter()
        .map(|n| name(&dom, n))
        .collect();
    assert_eq!(
        names,
        [
            "GraphOutput",
            "ClipNode",
            "ClipNode1",
            "StateMachineNode",
            "Blend1DNode",
            "MaskNode"
        ]
    );
    let run = kid(&dom, def, "ClipNode1");
    assert_eq!(prop(&dom, run, "NodeType"), Some(Variant::Enum(5)));
    assert_eq!(
        prop(&dom, run, "NodeId"),
        None,
        "script-locked; the editor leaves it"
    );
    assert_eq!(attr(&dom, run, "Speed"), text("param::speed"));
    assert_eq!(attr(&dom, run, "PlayMode"), Some(Variant::Enum(0)));
    assert_eq!(attr(&dom, run, "TrimEnd"), Some(Variant::Float64(0.)));
    let config = kid(&dom, run, "Configuration");
    assert_eq!(dom.get(config).unwrap().class(), "Configuration");
    assert_eq!(
        attr(&dom, config, "AnimGraph_Position"),
        Some(Variant::Vector2(Vector2Data { x: 120., y: -40. }))
    );
    assert_eq!(attr(&dom, config, "AnimGraph_DisplayName"), text("Run"));
    assert_eq!(
        attr(&dom, config, "AnimGraph_Collapsed"),
        Some(Variant::Bool(true))
    );
    let idle = kid(&dom, def, "ClipNode");
    let idle_config = kid(&dom, idle, "Configuration");
    assert_eq!(attr(&dom, idle_config, "AnimGraph_DisplayName"), None);
}

#[test]
fn expressions_are_value_nodes_wired_by_an_object_value() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let expr = kid(&dom, def, "Expression");
    assert_eq!(
        dom.get(expr).unwrap().class(),
        "AnimationValueNodeDefinition"
    );
    assert_eq!(prop(&dom, expr, "NodeType"), Some(Variant::Enum(1)));
    assert_eq!(attr(&dom, expr, "Expression"), text("speed / 10"));
    let output = children(&dom, expr, "AnimationValueOutputDefinition")[0];
    let blend = kid(&dom, def, "Blend1DNode");
    assert_eq!(
        attr(&dom, blend, "Position"),
        None,
        "the attribute is cleared"
    );
    let wire = kid(&dom, blend, "Position");
    assert_eq!(reference(&dom, wire, "Value"), Some(output));
    let a = kid(&dom, blend, "A");
    assert_eq!(attr(&dom, a, "Position"), text("param::speed"));
    let b = kid(&dom, blend, "B");
    assert_eq!(attr(&dom, b, "Position"), Some(Variant::Float64(1.)));
}

#[test]
fn state_machines_point_at_state_object_values() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let sm = kid(&dom, def, "StateMachineNode");
    let clip = kid(&dom, sm, "Clip");
    let run = kid(&dom, sm, "Run");
    assert_eq!(
        reference(&dom, clip, "Value"),
        Some(kid(&dom, def, "ClipNode"))
    );
    let run_config = kid(&dom, run, "Configuration");
    assert_eq!(
        attr(&dom, run_config, "AnimGraph_Position"),
        Some(Variant::Vector2(Vector2Data { x: 40., y: 60. }))
    );
    assert_eq!(
        attr(&dom, kid(&dom, sm, "Entry"), "RBX_IsStart"),
        Some(Variant::Bool(true))
    );
    assert_eq!(
        attr(&dom, kid(&dom, sm, "Any"), "RBX_IsAny"),
        Some(Variant::Bool(true))
    );
    assert_eq!(attr(&dom, sm, "EntryState"), text("Clip"));

    let go = kid(&dom, sm, "Clip_to_Run");
    assert_eq!(
        dom.get(go).unwrap().class(),
        "StateMachineTransitionDefinition"
    );
    assert_eq!(reference(&dom, go, "From"), Some(clip));
    assert_eq!(reference(&dom, go, "To"), Some(run));
    assert_eq!(prop(&dom, go, "Priority"), Some(Variant::Int32(3)));
    assert_eq!(
        attr(&dom, go, "Duration"),
        Some(Variant::Float64(f64::from(0.3f32)))
    );
    assert_eq!(attr(&dom, go, "Curve"), Some(Variant::Enum(5)));
    assert_eq!(attr(&dom, go, "WaitFor"), Some(Variant::Enum(1)));
    assert_eq!(attr(&dom, go, "When"), Some(Variant::Enum(0)));
    assert_eq!(attr(&dom, go, "Expression"), text("speed > 1"));
    let Some(Variant::String(id)) = prop(&dom, go, "TransitionId") else {
        panic!("no TransitionId");
    };
    let groups: Vec<usize> = id.split('-').map(str::len).collect();
    assert_eq!(groups, [8, 4, 4, 4, 12]);
    assert!(id
        .chars()
        .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));

    let back = kid(&dom, sm, "Any_to_Clip");
    assert_eq!(reference(&dom, back, "From"), None, "(Any)");
    assert_eq!(attr(&dom, back, "WaitFor"), Some(Variant::Enum(0)));
    assert_eq!(attr(&dom, back, "When"), Some(Variant::Enum(1)));
}

#[test]
fn masks_are_shared_folders_of_joint_weights() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let shared = kid(&dom, def, "SharedMasks");
    let mask = kid(&dom, shared, "UpperBody");
    assert_eq!(attr(&dom, mask, "RBX_MaskRigType"), text("HRD"));
    assert_eq!(
        attr(&dom, mask, "RightUpperArm"),
        Some(Variant::Float64(0.5))
    );
    let torso = kid(&dom, mask, "UpperTorso");
    assert_eq!(dom.get(torso).unwrap().class(), "NumberValue");
    assert_eq!(prop(&dom, torso, "Value"), Some(Variant::Float64(1.)));
    let arm = kid(&dom, torso, "RightUpperArm");
    assert_eq!(attr(&dom, arm, "Order"), Some(Variant::Float64(1.)));
    let node = kid(&dom, def, "MaskNode");
    assert_eq!(
        reference(&dom, kid(&dom, node, "Mask"), "Value"),
        Some(mask)
    );
    assert_eq!(attr(&dom, node, "Mask"), None);
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
    assert!(back.masks.is_empty());
}

#[test]
fn an_unparsed_pin_list_survives_only_while_the_pins_do() {
    let mut dom = WeakDom::new();
    let def = write(&mut dom, None, &sample());
    let sm = kid(&dom, def, "StateMachineNode");
    let blob = vec![1, 2, 0, 0xFF];
    let raw = Variant::Unknown {
        type_id: 0x01,
        raw: blob.clone(),
    };
    dom.set_property(sm, "InputPinData", raw.clone()).unwrap();
    let mut g = read(&dom, def);
    overwrite(&mut dom, def, &g);
    let sm = kid(&dom, def, "StateMachineNode");
    assert_eq!(prop(&dom, sm, "InputPinData"), Some(raw));
    let at = g
        .nodes
        .iter()
        .position(|n| n.kind == NodeKind::StateMachine)
        .unwrap();
    g.nodes[at].add_pin();
    overwrite(&mut dom, def, &g);
    let sm = kid(&dom, def, "StateMachineNode");
    assert_eq!(prop(&dom, sm, "InputPinData"), None);
}

#[test]
fn an_empty_definition_still_has_an_output() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "G", None);
    assert!(read(&dom, def).output().is_some());
}
