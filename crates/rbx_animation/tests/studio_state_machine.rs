//! A state machine saved by Roblox Studio (`AnimationGraphDefinition` "Graph":
//! a `StateMachineNode` with states State and State1 (the entry is State),
//! feeding the output). The writer must produce the bytes Studio did for
//! `EntryState` and `InputPinData`.

use rbx_animation::graph::dom::{read, write};
use rbx_animation::graph::model::{NodeKind, Prop, Value};
use rbx_dom::{Ref, Variant, WeakDom};

fn load() -> (WeakDom, Ref) {
    let path = format!(
        "{}/tests/fixtures/studio_state_machine.rbxm",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(path).unwrap();
    let dom = rbx_binary::deserialize(&bytes).unwrap();
    let def = dom.root_refs()[0];
    (dom, def)
}

fn machine(dom: &WeakDom, def: Ref) -> Ref {
    child(dom, def, "StateMachineNode")
}

fn child(dom: &WeakDom, of: Ref, name: &str) -> Ref {
    dom.get(of)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn raw(dom: &WeakDom, of: Ref, property: &str) -> Vec<u8> {
    match dom.get(of).unwrap().properties().get(property) {
        Some(Variant::Unknown { raw, .. }) => raw.clone(),
        Some(Variant::String(s)) => s.as_bytes().to_vec(),
        other => panic!("{property}: {other:?}"),
    }
}

#[test]
fn studio_s_entry_state_reads_as_the_state_it_names() {
    let (dom, def) = load();
    let graph = read(&dom, def);
    let sm = graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::StateMachine)
        .unwrap();
    assert_eq!(
        sm.props["EntryState"],
        Prop::Value(Value::Text("State".into()))
    );
    assert_eq!(sm.inputs.len(), 3);
}

#[test]
fn the_writer_makes_studio_s_entry_state_and_pin_data_bytes() {
    let (studio, def) = load();
    let graph = read(&studio, def);

    let mut dom = WeakDom::new();
    let out = write(&mut dom, None, &graph);
    let (a, b) = (machine(&studio, def), machine(&dom, out));

    assert_eq!(
        raw(&dom, b, "InputPinData"),
        raw(&studio, a, "InputPinData")
    );
    let ours =
        rbx_dom::attributes::decode(dom.get(b).unwrap().properties().get("AttributesSerialize"));
    let state = child(&dom, b, "State");
    assert_eq!(ours["EntryState"], Variant::Ref(state));

    // In a binary file the attribute holds the state's file referent; saved
    // and reloaded it must still name the same ObjectValue.
    let saved = rbx_binary::serialize(&dom).unwrap();
    let back = rbx_binary::deserialize(&saved).unwrap();
    let def = back.root_refs()[0];
    let sm = machine(&back, def);
    let theirs = rbx_dom::attributes::decode(
        back.get(sm)
            .unwrap()
            .properties()
            .get("AttributesSerialize"),
    );
    let Variant::Ref(target) = theirs["EntryState"] else {
        panic!("no entry");
    };
    assert_eq!(back.get(target).unwrap().name(), "State");
    assert_eq!(
        raw(&back, sm, "InputPinData"),
        raw(&studio, a, "InputPinData")
    );
}
