//! Coverage for the SSTR chunk writer.
//!
//! Real files mix `Variant::String` (a SharedString entry that decoded as valid
//! UTF-8, empty included) and `Variant::Unknown { type_id: 0x1C, .. }` (binary
//! content, e.g. mesh data) on sibling instances of the very same property, since
//! the wire type is uniform per property but the *content* of each entry decides
//! which shape the reader hands back. This pattern surfaced serialization bugs:
//! `serialize` was rejecting `Model.ModelMeshData` properties entirely, and there
//! was no SSTR chunk writer at all for SharedString-bearing properties.

mod support;

use rbx_binary::{deserialize, parse_header, read_chunks, serialize};
use rbx_dom::{Variant, WeakDom};

use support::all_refs;

const MESH_DATA_TYPE_ID: u8 = 0x1C;

/// A real, richly-populated place file, read only by the one test that needs
/// one — never baked into the binary, so the crate still builds for anyone
/// without it. Set `RBX_ROUND_TRIP_FIXTURE` to opt in.
fn real_place() -> Vec<u8> {
    let path = std::env::var("RBX_ROUND_TRIP_FIXTURE")
        .expect("set RBX_ROUND_TRIP_FIXTURE to a real .rbxl place file");
    std::fs::read(&path).expect("RBX_ROUND_TRIP_FIXTURE must be readable")
}

// Real mesh/collision data is binary, not text; a `0xFF` lead byte (never valid
// UTF-8) keeps these payloads exercising the `Variant::Unknown` path both before
// and after the round-trip, rather than accidentally decoding as a plain string.
fn mesh_data(tag: &[u8]) -> Variant {
    let mut raw = vec![0xFF];
    raw.extend_from_slice(tag);
    Variant::Unknown {
        type_id: MESH_DATA_TYPE_ID,
        raw,
    }
}

// Reads only the SSTR chunk's declared entry count (version i32 + count i32),
// exactly as `chunks::sstr::parse` frames it, without depending on that private
// parser: this test only needs to know how many entries the table ended up with.
fn sstr_entry_count(bytes: &[u8]) -> i32 {
    let (_, body) = parse_header(bytes).unwrap();
    let chunk = read_chunks(body)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .find(|chunk| chunk.name_str() == "SSTR")
        .expect("serialize always writes a SSTR chunk");

    i32::from_le_bytes(chunk.data[4..8].try_into().unwrap())
}

#[test]
fn identical_shared_string_payloads_dedup_to_one_sstr_entry() {
    let mut dom = WeakDom::new();
    for _ in 0..4 {
        let referent = dom.new_instance("Model", "M", None);
        dom.set_property(
            referent,
            "ModelMeshData",
            mesh_data(b"identical-mesh-bytes"),
        )
        .unwrap();
    }

    let bytes = serialize(&dom).unwrap();
    assert_eq!(
        sstr_entry_count(&bytes),
        1,
        "four instances sharing one payload must dedup to a single SSTR entry"
    );

    let after = deserialize(&bytes).unwrap();
    for referent in all_refs(&after) {
        let instance = after.get(referent).unwrap();
        assert_eq!(
            instance.properties().get("ModelMeshData"),
            Some(&mesh_data(b"identical-mesh-bytes"))
        );
    }
}

#[test]
fn distinct_shared_string_payloads_get_two_sstr_entries() {
    let mut dom = WeakDom::new();
    let a = dom.new_instance("Model", "A", None);
    dom.set_property(a, "ModelMeshData", mesh_data(b"payload-a"))
        .unwrap();
    let b = dom.new_instance("Model", "B", None);
    dom.set_property(b, "ModelMeshData", mesh_data(b"payload-b-is-longer"))
        .unwrap();

    let bytes = serialize(&dom).unwrap();
    assert_eq!(sstr_entry_count(&bytes), 2);

    let after = deserialize(&bytes).unwrap();
    let get = |referent| {
        after
            .get(referent)
            .unwrap()
            .properties()
            .get("ModelMeshData")
            .cloned()
    };
    assert_eq!(get(a), Some(mesh_data(b"payload-a")));
    assert_eq!(get(b), Some(mesh_data(b"payload-b-is-longer")));
}

#[test]
fn a_shared_string_property_mixed_with_plain_empty_strings_round_trips() {
    // Mixed shapes: some instances have real binary content, others empty strings
    // for the same SharedString property.
    let mut dom = WeakDom::new();
    let with_data = dom.new_instance("Model", "WithData", None);
    dom.set_property(with_data, "ModelMeshData", mesh_data(b"real-mesh-bytes"))
        .unwrap();
    let without_data = dom.new_instance("Model", "WithoutData", None);
    dom.set_property(
        without_data,
        "ModelMeshData",
        Variant::String(String::new()),
    )
    .unwrap();

    let bytes = serialize(&dom).expect("mixed String/Unknown shapes must serialize");
    let after = deserialize(&bytes).unwrap();

    assert_eq!(
        after
            .get(with_data)
            .unwrap()
            .properties()
            .get("ModelMeshData"),
        Some(&mesh_data(b"real-mesh-bytes"))
    );
    assert_eq!(
        after
            .get(without_data)
            .unwrap()
            .properties()
            .get("ModelMeshData"),
        Some(&Variant::String(String::new()))
    );
}

#[test]
#[ignore = "needs RBX_ROUND_TRIP_FIXTURE"]
fn a_real_place_round_trips_through_serialize() {
    let before = deserialize(&real_place()).unwrap();
    let bytes = serialize(&before).expect("real place must serialize");
    let after = deserialize(&bytes).unwrap();

    let before_refs = all_refs(&before);
    assert_eq!(
        before_refs.len(),
        all_refs(&after).len(),
        "instance count changed"
    );

    // Proves the test actually exercises the bug `rbxdump --roundtrip` found:
    // at least one instance must carry real (non-empty) SharedString content.
    let mut exercised_non_empty_mesh_data = 0;

    for referent in before_refs {
        let original = before.get(referent).unwrap();
        let round_tripped = after.get(referent).unwrap_or_else(|| {
            panic!(
                "{referent:?} ({}) missing after round-trip",
                original.class()
            )
        });

        assert_eq!(original.class(), round_tripped.class());
        assert_eq!(original.name(), round_tripped.name());
        assert_eq!(
            original.properties(),
            round_tripped.properties(),
            "{} {:?} properties changed",
            original.class(),
            original.name()
        );

        if let Some(Variant::Unknown { type_id, .. }) = original.properties().get("ModelMeshData") {
            if *type_id == MESH_DATA_TYPE_ID {
                exercised_non_empty_mesh_data += 1;
            }
        }
    }

    assert!(
        exercised_non_empty_mesh_data > 0,
        "fixture is expected to carry at least one non-empty ModelMeshData instance"
    );
}
