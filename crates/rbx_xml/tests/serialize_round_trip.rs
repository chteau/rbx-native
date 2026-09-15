//! Round-trips the real `TestPlace.rbxl` fixture through
//! `rbx_binary::deserialize -> rbx_xml::serialize -> rbx_xml::deserialize` and
//! checks the two DOMs are equal instance by instance: class, name, parent, and
//! every property value.
//!
//! Unlike `rbx_binary`'s own binary-format round-trip test, this cannot compare
//! by identical `Ref` value: the XML reader assigns brand new referents in file
//! order on every parse (see `deserializer::assign_referents`), so a `Ref` that
//! was `7` in the fixture may come back as a different number here. Instead,
//! both trees are walked in the same deterministic pre-order (root list, then
//! each instance's `children()` in order — both of which this crate's writer
//! and reader preserve), and instances are paired up by position in that walk.

use std::collections::HashMap;
use std::path::Path;

use rbx_dom::{Content, Instance, Ref, Variant, WeakDom};

const TEST_PLACE: &[u8] = include_bytes!("../../../assets/tests/TestPlace.rbxl");

fn preorder(dom: &WeakDom) -> Vec<Ref> {
    fn visit(dom: &WeakDom, r: Ref, out: &mut Vec<Ref>) {
        out.push(r);
        for &child in dom.get(r).unwrap().children() {
            visit(dom, child, out);
        }
    }
    let mut out = Vec::new();
    for &root in dom.root_refs() {
        visit(dom, root, &mut out);
    }
    out
}

// Rewrites the `Ref`s embedded inside a property value through `map`, so a value
// from the original tree can be compared against its round-tripped counterpart
// even though the two trees disagree on raw `Ref` numbering.
fn remap(value: &Variant, map: &HashMap<Ref, Ref>) -> Variant {
    match value {
        Variant::Ref(r) => Variant::Ref(map[r]),
        Variant::Content(Content::Object(r)) => Variant::Content(Content::Object(map[r])),
        other => other.clone(),
    }
}

#[test]
fn test_place_round_trips_through_serialize() {
    let before = rbx_binary::deserialize(TEST_PLACE).unwrap();
    let xml = rbx_xml::serialize(&before).unwrap();
    let after = rbx_xml::deserialize(&xml).unwrap();

    let before_order = preorder(&before);
    let after_order = preorder(&after);
    assert_eq!(
        before_order.len(),
        after_order.len(),
        "instance count changed"
    );

    let map: HashMap<Ref, Ref> = before_order
        .iter()
        .copied()
        .zip(after_order.iter().copied())
        .collect();

    for (&original_ref, &round_tripped_ref) in before_order.iter().zip(after_order.iter()) {
        let original = before.get(original_ref).unwrap();
        let round_tripped = after.get(round_tripped_ref).unwrap();

        assert_eq!(
            original.class(),
            round_tripped.class(),
            "{original_ref:?}: class changed"
        );
        assert_eq!(
            original.name(),
            round_tripped.name(),
            "{original_ref:?} ({}): name changed",
            original.class()
        );

        let expected_properties: HashMap<&String, Variant> = original
            .properties()
            .iter()
            .map(|(name, value)| (name, remap(value, &map)))
            .collect();
        let actual_properties: HashMap<&String, &Variant> =
            round_tripped.properties().iter().collect();
        assert_eq!(
            expected_properties.len(),
            actual_properties.len(),
            "{original_ref:?} ({} {:?}): property count changed",
            original.class(),
            original.name()
        );
        for (name, expected) in &expected_properties {
            assert_eq!(
                actual_properties.get(name),
                Some(&expected),
                "{original_ref:?} ({} {:?}): property {name} changed",
                original.class(),
                original.name()
            );
        }

        let expected_parent = original_parent(&before, original_ref).map(|p| map[&p]);
        let actual_parent = original_parent(&after, round_tripped_ref);
        assert_eq!(
            expected_parent,
            actual_parent,
            "{original_ref:?} ({} {:?}): parent changed",
            original.class(),
            original.name()
        );
    }

    let scratch = Path::new(
        "/tmp/claude-1000/-mnt-data-Documents-Dev/02662147-12e3-4e0e-a3a2-acc15eb8564c/scratchpad/xml_serialize",
    );
    if scratch.is_dir() {
        std::fs::write(scratch.join("TestPlace.rbxlx"), &xml).unwrap();
    }
    eprintln!(
        "TestPlace.rbxl: {} bytes binary -> {} bytes rbxlx, {} instances",
        TEST_PLACE.len(),
        xml.len(),
        before_order.len()
    );
}

// `WeakDom` has no direct parent lookup; root instances have no parent, and
// every other instance's parent is whichever instance lists it in `children()`.
fn original_parent(dom: &WeakDom, referent: Ref) -> Option<Ref> {
    if dom.root_refs().contains(&referent) {
        return None;
    }
    preorder(dom)
        .into_iter()
        .find(|&candidate| dom.get(candidate).unwrap().children().contains(&referent))
}

// `STRING_TYPE_ID` (0x01): the one `Unknown` type_id the XML `BinaryString` tag
// can carry losslessly, since a read-back blob is always reconstructed with
// that same hardcoded id (see `value::scalar::binary_string`).
#[test]
fn unknown_blob_with_string_type_id_round_trips_losslessly() {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Terrain", "Terrain"));
    let raw = vec![0xDE, 0xAD, 0xBE, 0xEF];
    dom.set_property(
        Ref::new(1),
        "SmoothGrid",
        Variant::Unknown {
            type_id: 1,
            raw: raw.clone(),
        },
    )
    .unwrap();

    let xml = rbx_xml::serialize(&dom).unwrap();
    let after = rbx_xml::deserialize(&xml).unwrap();
    let got = after.get(after.root_refs()[0]).unwrap();
    assert_eq!(
        got.properties().get("SmoothGrid"),
        Some(&Variant::Unknown { type_id: 1, raw })
    );
}

// A `type_id` other than `STRING_TYPE_ID` has no faithful XML representation
// (see `serializer::value::encode`): writing it as `BinaryString` would read
// back tagged 1 instead of its real id, silently corrupting the property.
// `serialize` must refuse it rather than produce that corrupted file.
#[test]
fn unknown_blob_with_other_type_id_is_rejected_instead_of_corrupted() {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Part", "Part"));
    dom.set_property(
        Ref::new(1),
        "PhysicalConfigData",
        Variant::Unknown {
            type_id: 28,
            raw: vec![0x01, 0x02, 0x03],
        },
    )
    .unwrap();

    let err = rbx_xml::serialize(&dom).unwrap_err();
    assert!(
        err.to_string().contains("Unknown"),
        "unexpected error: {err}"
    );
}
