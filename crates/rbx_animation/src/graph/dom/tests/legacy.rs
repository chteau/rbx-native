//! Graphs saved in this crate's earlier layout still read.

use std::collections::BTreeMap;

use rbx_dom::{attributes, Ref, Variant, Vector2Data, WeakDom};

use super::super::*;
use crate::graph::model::{NodeKind, ParamKind, Prop, Value, WaitFor};

fn put(dom: &mut WeakDom, of: Ref, map: &[(&str, Variant)]) {
    let map: BTreeMap<String, Variant> = map
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    let raw = attributes::encode(&map, |_| None).unwrap();
    dom.set_property(of, ATTRIBUTES, Variant::Unknown { type_id: 0x01, raw })
        .unwrap();
}

fn node(dom: &mut WeakDom, def: Ref, name: &str, id: &str, kind: NodeKind) -> Ref {
    let r = dom.new_instance("AnimationNodeDefinition", name, Some(def));
    dom.set_property(r, "NodeId", Variant::String(id.into()))
        .unwrap();
    dom.set_property(r, "NodeType", Variant::Enum(kind.value()))
        .unwrap();
    r
}

fn pin(dom: &mut WeakDom, of: Ref, name: &str, to: Ref) -> Ref {
    let p = dom.new_instance("ObjectValue", name, Some(of));
    dom.set_property(p, "Value", Variant::Ref(to)).unwrap();
    p
}

#[test]
fn reads_the_earlier_layout() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "Old", None);
    put(
        &mut dom,
        def,
        &[
            ("jump", Variant::Bool(false)),
            ("speed", Variant::Float64(2.)),
        ],
    );
    dom.set_property(
        def,
        "Tags",
        Variant::String(attributes::encode_tags(&["Trigger:jump"])),
    )
    .unwrap();
    let idle = node(&mut dom, def, "Idle", "Clip1", NodeKind::Clip);
    put(
        &mut dom,
        idle,
        &[
            (
                "EditorPosition",
                Variant::Vector2(Vector2Data { x: 5., y: 6. }),
            ),
            ("PlayMode", Variant::String("OnceAndHold".into())),
            ("Speed", Variant::String("$speed".into())),
        ],
    );
    let run = node(&mut dom, def, "Run", "Clip2", NodeKind::Clip);
    put(
        &mut dom,
        run,
        &[("Speed", Variant::String("=speed * 2".into()))],
    );
    let sm = node(
        &mut dom,
        def,
        "State Machine",
        "StateMachine1",
        NodeKind::StateMachine,
    );
    put(
        &mut dom,
        sm,
        &[("EntryState", Variant::String("State2".into()))],
    );
    pin(&mut dom, sm, "State1", idle);
    pin(&mut dom, sm, "State2", run);
    let t = dom.new_instance("StateMachineTransitionDefinition", "Transition", Some(sm));
    dom.set_property(t, "To", Variant::Ref(run)).unwrap();
    dom.set_property(t, "From", Variant::Ref(idle)).unwrap();
    put(
        &mut dom,
        t,
        &[
            ("WaitFor", Variant::String("Finished".into())),
            ("Curve", Variant::String("CubicV2".into())),
        ],
    );
    let out = node(&mut dom, def, "Output", "Output", NodeKind::GraphOutput);
    pin(&mut dom, out, "Pose", sm);

    let g = read(&dom, def);
    assert_eq!(g.param("jump").unwrap().kind, ParamKind::Trigger);
    assert_eq!(g.param("speed").unwrap().default, Value::Number(2.));
    assert_eq!(g.nodes[0].id, "Clip1");
    assert_eq!(g.nodes[0].name, "Idle");
    assert_eq!(g.nodes[0].position, (5., 6.));
    assert_eq!(g.nodes[0].props["Speed"], Prop::Param("speed".into()));
    assert_eq!(
        g.nodes[0].props["PlayMode"],
        Prop::Value(Value::Text("OnceAndHold".into()))
    );
    assert_eq!(g.nodes[1].props["Speed"], Prop::Expr("speed * 2".into()));
    let machine = &g.nodes[2];
    assert_eq!(machine.inputs.len(), 2);
    assert_eq!(
        machine.props["EntryState"],
        Prop::Value(Value::Text("State2".into()))
    );
    let tr = &machine.transitions[0];
    assert_eq!(
        (tr.from, tr.to, tr.wait_for),
        (Some(0), 1, WaitFor::Finished)
    );
    assert_eq!(tr.curve, crate::easing::Style::CubicV2);
    assert_eq!(g.output(), Some(3));

    // Saving it moves the trigger into its folder and drops the tag.
    overwrite(&mut dom, def, &g);
    assert!(dom.get(def).unwrap().tags().is_empty());
    assert_eq!(
        read(&dom, def).param("jump").unwrap().kind,
        ParamKind::Trigger
    );
}

#[test]
fn entry_state_may_point_at_the_state_or_its_node() {
    let mut dom = WeakDom::new();
    let def = dom.new_instance("AnimationGraphDefinition", "G", None);
    let a = node(&mut dom, def, "ClipNode", "", NodeKind::Clip);
    let b = node(&mut dom, def, "ClipNode1", "", NodeKind::Clip);
    let sm = node(
        &mut dom,
        def,
        "StateMachineNode",
        "",
        NodeKind::StateMachine,
    );
    for r in [a, b, sm] {
        dom.new_instance("Configuration", "Configuration", Some(r));
    }
    pin(&mut dom, sm, "A", a);
    let second = pin(&mut dom, sm, "B", b);
    dom.set_property(sm, "EntryState", Variant::Ref(second))
        .unwrap();
    let entry = |dom: &WeakDom| read(dom, def).nodes[2].props["EntryState"].clone();
    assert_eq!(entry(&dom), Prop::Value(Value::Text("B".into())));
    dom.set_property(sm, "EntryState", Variant::Ref(a)).unwrap();
    assert_eq!(entry(&dom), Prop::Value(Value::Text("A".into())));
}
