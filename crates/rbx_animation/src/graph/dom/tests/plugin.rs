//! A graph built instance by instance the way the shipping Graph Editor
//! plugin writes one, then read: the layout `dom` documents, checked from
//! the file's side rather than this crate's writer.

use std::collections::BTreeMap;

use rbx_dom::{attributes, Ref, Variant, Vector2Data, WeakDom};

use super::super::*;
use crate::easing::Style;
use crate::graph::model::enums;
use crate::graph::model::{NodeKind, ParamKind, Prop, Value, WaitFor, When};

/// Sets `of`'s attributes, naming each `EnumItem`'s enum by the property
/// (or, for the `Play` parameter, the enum its type stands for).
fn put(dom: &mut WeakDom, of: Ref, map: &[(&str, Variant)]) {
    let map: BTreeMap<String, Variant> = map
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    let raw = attributes::encode(&map, |n| match n {
        "Play" => Some(enums::PLAY_MODE.name.to_owned()),
        other => enums::of_prop(other).map(|e| e.name.to_owned()),
    })
    .unwrap();
    dom.set_property(of, ATTRIBUTES, Variant::Unknown { type_id: 0x01, raw })
        .unwrap();
}

fn object(dom: &mut WeakDom, class: &str, name: &str, parent: Ref) -> Ref {
    dom.new_instance(class, name, Some(parent))
}

fn link(dom: &mut WeakDom, of: Ref, property: &str, to: Ref) {
    dom.set_property(of, property, Variant::Ref(to)).unwrap();
}

/// An `AnimationNodeDefinition` as `createNodeOfType` makes it: a type, no
/// `NodeId`, and a `Configuration` holding its place.
fn node(dom: &mut WeakDom, def: Ref, name: &str, kind: NodeKind, at: (f32, f32)) -> Ref {
    let r = object(dom, "AnimationNodeDefinition", name, def);
    dom.set_property(r, "NodeType", Variant::Enum(kind.value()))
        .unwrap();
    layout(dom, r, at, &[]);
    r
}

fn layout(dom: &mut WeakDom, of: Ref, at: (f32, f32), extra: &[(&str, Variant)]) {
    let config = object(dom, "Configuration", "Configuration", of);
    let mut map = vec![(
        "AnimGraph_Position",
        Variant::Vector2(Vector2Data { x: at.0, y: at.1 }),
    )];
    map.extend(extra.iter().cloned());
    put(dom, config, &map);
}

fn pin(dom: &mut WeakDom, of: Ref, name: &str, source: Option<Ref>) -> Ref {
    let p = object(dom, "ObjectValue", name, of);
    if let Some(source) = source {
        link(dom, p, "Value", source);
    }
    p
}

fn parameter(dom: &mut WeakDom, def: Ref, name: &str, kind: &str) -> Ref {
    let folder = object(dom, "Folder", &format!("Parameter_{name}"), def);
    put(
        dom,
        folder,
        &[
            ("AnimGraph_ParameterType", Variant::String(kind.into())),
            ("AnimGraph_BindingName", Variant::String(name.into())),
        ],
    );
    layout(dom, folder, (-200., 0.), &[]);
    folder
}

fn number(n: f64) -> Variant {
    Variant::Float64(n)
}

fn text(s: &str) -> Variant {
    Variant::String(s.into())
}

/// The Locomotion graph from Roblox's state-machine walkthrough, saved as
/// the editor saves it.
fn saved() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let dom_ref = &mut dom;
    let def = dom_ref.new_instance("AnimationGraphDefinition", "Locomotion", None);
    put(
        dom_ref,
        def,
        &[
            ("Speed", number(0.)),
            ("Grounded", Variant::Bool(true)),
            ("Jump", Variant::Bool(false)),
            ("Mode", text("walk")),
            ("Play", Variant::Enum(1)),
            ("RBX_GraphDirty", Variant::Bool(true)),
            ("RBX_GraphVersion", number(4.)),
            ("RBX_SourceAssetId", number(123.)),
        ],
    );

    let output = node(dom_ref, def, "GraphOutput", NodeKind::GraphOutput, (0., 0.));
    let idle = node(dom_ref, def, "ClipNode", NodeKind::Clip, (-600., 0.));
    put(
        dom_ref,
        idle,
        &[
            ("AnimationId", text("rbxassetid://507766666")),
            ("PlayMode", Variant::Enum(0)),
            ("Speed", text("param::Speed")),
            ("Trim", Variant::Bool(false)),
        ],
    );
    let fall = node(dom_ref, def, "ClipNode1", NodeKind::Clip, (-600., 200.));
    put(dom_ref, fall, &[("PlayMode", Variant::Enum(2))]);
    let config = dom_ref.get(fall).unwrap().children()[0];
    put(
        dom_ref,
        config,
        &[
            (
                "AnimGraph_Position",
                Variant::Vector2(Vector2Data { x: -600., y: 200. }),
            ),
            ("AnimGraph_DisplayName", text("Fall")),
            ("AnimGraph_Collapsed", Variant::Bool(true)),
            ("AnimGraph_ZIndex", number(2.)),
        ],
    );

    // Blend1D seeded with A and B, Position driven by an expression.
    let blend = node(dom_ref, def, "Blend1DNode", NodeKind::Blend1D, (-300., 0.));
    put(dom_ref, blend, &[("PhaseSync", Variant::Enum(0))]);
    let a = pin(dom_ref, blend, "A", Some(idle));
    put(dom_ref, a, &[("Position", number(0.))]);
    let b = pin(dom_ref, blend, "B", Some(fall));
    put(dom_ref, b, &[("Position", number(1.))]);
    let expr = object(dom_ref, "AnimationValueNodeDefinition", "Expression", def);
    dom_ref
        .set_property(expr, "NodeType", Variant::Enum(1))
        .unwrap();
    put(
        dom_ref,
        expr,
        &[("Expression", text("math.clamp(Speed / 16, 0, 1)"))],
    );
    layout(dom_ref, expr, (-500., -150.), &[]);
    let out = object(
        dom_ref,
        "AnimationValueOutputDefinition",
        "AnimationValueOutputDefinition",
        expr,
    );
    pin(dom_ref, blend, "Position", Some(out));

    // A state machine: two states, the pseudo-states, an entry and two
    // transitions.
    let sm = node(
        dom_ref,
        def,
        "StateMachineNode",
        NodeKind::StateMachine,
        (-100., 0.),
    );
    let ground = pin(dom_ref, sm, "Locomotion", Some(blend));
    layout(dom_ref, ground, (0., 0.), &[]);
    let air = pin(dom_ref, sm, "Fall", Some(fall));
    layout(dom_ref, air, (200., 0.), &[]);
    let entry = pin(dom_ref, sm, "(Entry)", None);
    put(dom_ref, entry, &[("RBX_IsStart", Variant::Bool(true))]);
    let any = pin(dom_ref, sm, "(Any)", None);
    put(dom_ref, any, &[("RBX_IsAny", Variant::Bool(true))]);
    put(dom_ref, sm, &[("EntryState", text("Locomotion"))]);
    let leave = object(
        dom_ref,
        "StateMachineTransitionDefinition",
        "Locomotion_to_Fall",
        sm,
    );
    link(dom_ref, leave, "From", ground);
    link(dom_ref, leave, "To", air);
    dom_ref
        .set_property(leave, "Priority", Variant::Int32(1))
        .unwrap();
    dom_ref
        .set_property(
            leave,
            "TransitionId",
            text("0C5F2A4E-3B1D-4E5F-9A7B-1C2D3E4F5A6B"),
        )
        .unwrap();
    put(
        dom_ref,
        leave,
        &[
            ("Duration", number(0.2)),
            ("Curve", Variant::Enum(0)),
            ("Expression", text("not Grounded")),
            ("WaitFor", Variant::Enum(1)),
            ("When", Variant::Enum(0)),
        ],
    );
    let land = object(
        dom_ref,
        "StateMachineTransitionDefinition",
        "Any_to_Locomotion",
        sm,
    );
    link(dom_ref, land, "To", ground);
    dom_ref
        .set_property(land, "Priority", Variant::Int32(2))
        .unwrap();
    dom_ref
        .set_property(
            land,
            "TransitionId",
            text("6D1E0B7A-92C4-4F31-8E55-0A9B8C7D6E5F"),
        )
        .unwrap();
    put(
        dom_ref,
        land,
        &[
            ("Duration", number(0.1)),
            ("Curve", Variant::Enum(5)),
            ("Expression", text("")),
            ("WaitFor", Variant::Enum(0)),
            ("When", Variant::Enum(1)),
        ],
    );

    // A shared mask, and a Mask node using it.
    let shared = object(dom_ref, "Folder", "SharedMasks", def);
    let mask = object(dom_ref, "Folder", "Mask", shared);
    put(
        dom_ref,
        mask,
        &[
            ("RBX_MaskRigType", text("HRD")),
            ("RBX_MaskRigName", text("")),
            ("UpperTorso", number(1.)),
            ("Head", number(0.25)),
        ],
    );
    let torso = object(dom_ref, "NumberValue", "UpperTorso", mask);
    dom_ref.set_property(torso, "Value", number(1.)).unwrap();
    put(dom_ref, torso, &[("Order", number(0.))]);
    let head = object(dom_ref, "NumberValue", "Head", torso);
    dom_ref.set_property(head, "Value", number(0.25)).unwrap();
    put(dom_ref, head, &[("Order", number(1.))]);
    let masked = node(dom_ref, def, "MaskNode", NodeKind::Mask, (100., 0.));
    put(dom_ref, masked, &[("Invert", Variant::Bool(true))]);
    pin(dom_ref, masked, "Pose", Some(sm));
    pin(dom_ref, masked, "Mask", Some(mask));
    pin(dom_ref, output, "Pose", Some(masked));

    // A Priority Select with a per-input trigger bound to a Trigger
    // parameter and an overridden transition.
    let pick = node(
        dom_ref,
        def,
        "PrioritySelectNode",
        NodeKind::PrioritySelect,
        (0., 300.),
    );
    put(
        dom_ref,
        pick,
        &[
            ("DefaultInterruptible", Variant::Enum(1)),
            ("DefaultTransitionDuration", number(0.1)),
            ("DefaultTransitionCurve", Variant::Enum(0)),
        ],
    );
    let first = pin(dom_ref, pick, "Clip", Some(idle));
    put(
        dom_ref,
        first,
        &[
            ("Trigger", text("param::Jump")),
            ("TransitionOverrideDuration", number(0.4)),
            ("TransitionOverrideInterruptible", Variant::Enum(2)),
        ],
    );
    pin(dom_ref, pick, "Clip1", None);

    // Parameter pills: Speed has two, each binding drawn by a hint.
    let speed = parameter(dom_ref, def, "Speed", "Number");
    let hint = object(dom_ref, "ObjectValue", "connectionHint_Speed", speed);
    link(dom_ref, hint, "Value", idle);
    parameter(dom_ref, def, "Speed", "Number");
    parameter(dom_ref, def, "Grounded", "Boolean");
    let jump = parameter(dom_ref, def, "Jump", "Trigger");
    let hint = object(dom_ref, "ObjectValue", "connectionHint_Trigger", jump);
    link(dom_ref, hint, "Value", first);
    parameter(dom_ref, def, "Mode", "String");
    parameter(dom_ref, def, "Play", "PlayMode");
    parameter(dom_ref, def, "Body", "Mask");
    (dom, def)
}

#[test]
fn reads_what_the_editor_saves() {
    let (dom, def) = saved();
    let g = read(&dom, def);
    assert_eq!(g.name, "Locomotion");

    let kinds: Vec<(&str, ParamKind, Value)> = g
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.default.clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Speed", ParamKind::Number, Value::Number(0.)),
            ("Grounded", ParamKind::Bool, Value::Bool(true)),
            ("Jump", ParamKind::Trigger, Value::Bool(false)),
            ("Mode", ParamKind::Text, Value::Text("walk".into())),
            ("Play", ParamKind::PlayMode, Value::Text("PingPong".into())),
            ("Body", ParamKind::Mask, Value::Text(String::new())),
        ]
    );
    assert_eq!(g.param_refs.len(), 7, "one pill per folder");
    let rbx: Vec<&str> = g.rbx_attributes.keys().map(String::as_str).collect();
    assert_eq!(
        rbx,
        ["RBX_GraphDirty", "RBX_GraphVersion", "RBX_SourceAssetId"]
    );

    let at = |id: &str| g.nodes.iter().position(|n| n.id == id).unwrap();
    let idle = &g.nodes[at("ClipNode")];
    assert_eq!(idle.name, "Clip", "no display name: the type's label");
    assert_eq!(idle.position, (-600., 0.));
    assert_eq!(idle.props["Speed"], Prop::Param("Speed".into()));
    assert_eq!(
        idle.props["PlayMode"],
        Prop::Value(Value::Text("Loop".into()))
    );
    let fall = &g.nodes[at("ClipNode1")];
    assert_eq!(
        (fall.name.as_str(), fall.collapsed, fall.z_index),
        ("Fall", true, 2.)
    );
    assert_eq!(
        fall.props["PlayMode"],
        Prop::Value(Value::Text("OnceAndHold".into()))
    );

    let blend = &g.nodes[at("Blend1DNode")];
    let pins: Vec<(&str, Option<usize>)> = blend
        .inputs
        .iter()
        .map(|i| (i.pin.as_str(), i.node))
        .collect();
    assert_eq!(
        pins,
        [("A", Some(at("ClipNode"))), ("B", Some(at("ClipNode1")))]
    );
    assert_eq!(
        blend.props["Position"],
        Prop::Expr("math.clamp(Speed / 16, 0, 1)".into())
    );
    assert_eq!(g.expressions.len(), 1);
    assert_eq!(g.expressions[0].position, (-500., -150.));

    let sm = &g.nodes[at("StateMachineNode")];
    let states: Vec<&str> = sm.inputs.iter().map(|i| i.pin.as_str()).collect();
    assert_eq!(states, ["Locomotion", "Fall"], "pseudo-states are no pins");
    assert_eq!(sm.inputs[1].position, Some((200., 0.)));
    assert_eq!(
        sm.props["EntryState"],
        Prop::Value(Value::Text("Locomotion".into()))
    );
    let leave = &sm.transitions[0];
    assert_eq!((leave.from, leave.to, leave.priority), (Some(0), 1, 1));
    assert_eq!(leave.expression, "not Grounded");
    assert_eq!(leave.wait_for, WaitFor::Trigger);
    assert_eq!(leave.id, "0C5F2A4E-3B1D-4E5F-9A7B-1C2D3E4F5A6B");
    let land = &sm.transitions[1];
    assert_eq!((land.from, land.to, land.priority), (None, 0, 2));
    assert_eq!(
        (land.wait_for, land.when, land.curve),
        (WaitFor::Finished, When::BeforeFinished, Style::CubicV2)
    );
    assert!((land.duration - 0.1).abs() < 1e-6);

    let masked = &g.nodes[at("MaskNode")];
    assert_eq!(
        masked.props["Mask"],
        Prop::Value(Value::Text("Mask".into()))
    );
    assert_eq!(masked.props["Invert"], Prop::Value(Value::Bool(true)));
    assert_eq!(g.masks[0].weight("Head"), Some(0.25));
    assert_eq!(g.masks[0].joints[0].children[0].name, "Head");

    let pick = &g.nodes[at("PrioritySelectNode")];
    assert_eq!(
        pick.props["DefaultInterruptible"],
        Prop::Value(Value::Text("Finished".into()))
    );
    let first = &pick.inputs[0];
    assert_eq!(first.props["Trigger"], Prop::Param("Jump".into()));
    assert_eq!(
        first.props["TransitionOverrideInterruptible"],
        Prop::Value(Value::Text("Trigger".into()))
    );
    assert_eq!(pick.inputs[1].node, None, "an unwired dynamic pin stays");
    assert_eq!(
        g.nodes[g.output().unwrap()].input("Pose"),
        Some(at("MaskNode"))
    );
}

#[test]
fn what_the_editor_saves_survives_a_save() {
    let (dom, def) = saved();
    let first = read(&dom, def);
    let mut again = WeakDom::new();
    let written = write(&mut again, None, &first);
    assert_eq!(read(&again, written), first);
}

#[test]
fn a_mask_may_live_on_its_object_value() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "G", None);
    let masked = node(&mut dom, def, "MaskNode", NodeKind::Mask, (0., 0.));
    let direct = pin(&mut dom, masked, "Mask", None);
    put(&mut dom, direct, &[("LeftUpperArm", number(0.5))]);
    let g = read(&dom, def);
    let mask = g.mask("Mask").expect("made a shared mask of it");
    assert_eq!(mask.weight("LeftUpperArm"), Some(0.5));
    let at = g
        .nodes
        .iter()
        .position(|n| n.kind == NodeKind::Mask)
        .unwrap();
    assert_eq!(
        g.nodes[at].props["Mask"],
        Prop::Value(Value::Text("Mask".into()))
    );
}

#[test]
fn an_entry_transition_and_object_value_ends_read_as_roblox_documents_them() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "G", None);
    let clip = node(&mut dom, def, "ClipNode", NodeKind::Clip, (0., 0.));
    let sm = node(
        &mut dom,
        def,
        "StateMachineNode",
        NodeKind::StateMachine,
        (0., 0.),
    );
    let one = pin(&mut dom, sm, "One", None);
    let two = pin(&mut dom, sm, "Two", Some(clip));
    let entry = pin(&mut dom, sm, "(Entry)", None);
    put(&mut dom, entry, &[("RBX_IsStart", Variant::Bool(true))]);
    let start = object(&mut dom, "StateMachineTransitionDefinition", "Entry", sm);
    link(&mut dom, start, "From", entry);
    link(&mut dom, start, "To", two);
    let t = object(&mut dom, "StateMachineTransitionDefinition", "T", sm);
    pin(&mut dom, t, "From", Some(clip));
    pin(&mut dom, t, "To", Some(one));
    let g = read(&dom, def);
    let at = g
        .nodes
        .iter()
        .position(|n| n.kind == NodeKind::StateMachine)
        .unwrap();
    let sm = &g.nodes[at];
    assert_eq!(
        sm.props["EntryState"],
        Prop::Value(Value::Text("Two".into()))
    );
    let ends: Vec<_> = sm.transitions.iter().map(|t| (t.from, t.to)).collect();
    assert_eq!(ends, [(Some(1), 0)], "From names the node feeding Two");
}
