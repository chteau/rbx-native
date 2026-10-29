//! Studio resolves an `Instance` attribute while it reads the holder's PROP
//! chunk, so a target whose INST chunk comes later loads as an unresolved
//! `InstanceHandle`. Every INST chunk has to come before the first PROP.

use std::collections::BTreeMap;

use rbx_binary::{parse_header, read_chunks, serialize};
use rbx_dom::{attributes, Variant, WeakDom};

#[test]
fn every_inst_chunk_comes_before_the_first_prop_chunk() {
    let mut dom = WeakDom::new();
    // The holder's class is seen first, so it is the first INST chunk and the
    // target's class comes after it.
    let holder = dom.new_instance("AnimationNodeDefinition", "StateMachineNode", None);
    let target = dom.new_instance("ObjectValue", "Clip", Some(holder));
    let entry = BTreeMap::from([("EntryState".to_owned(), Variant::Ref(target))]);
    let blob = attributes::encode(&entry, |_| None).unwrap();
    dom.set_property(
        holder,
        "AttributesSerialize",
        Variant::String(String::from_utf8(blob).unwrap()),
    )
    .unwrap();

    let bytes = serialize(&dom).unwrap();
    let (_, body) = parse_header(&bytes).unwrap();
    let names: Vec<String> = read_chunks(body)
        .map(|chunk| chunk.unwrap().name_str().to_owned())
        .collect();

    let last_inst = names.iter().rposition(|n| n == "INST").unwrap();
    let first_prop = names.iter().position(|n| n == "PROP").unwrap();
    assert!(last_inst < first_prop, "{names:?}");
}
