//! Round-trips the real fixtures through `deserialize -> serialize -> deserialize` and
//! checks the two DOMs are equal instance by instance: class, name, parent, and every
//! property value (including raw `Unknown` blobs).

mod support;

use std::path::Path;

use rbx_binary::{deserialize, serialize};
use rbx_dom::WeakDom;

use support::{all_refs, parent_of, FPS, TEST_PLACE};

fn assert_same_tree(before: &WeakDom, after: &WeakDom, label: &str) {
    let before_refs = all_refs(before);
    let after_refs = all_refs(after);
    assert_eq!(
        before_refs.len(),
        after_refs.len(),
        "{label}: instance count changed"
    );

    for referent in before_refs {
        let original = before
            .get(referent)
            .unwrap_or_else(|| panic!("{label}: {referent:?} missing from the original DOM"));
        let round_tripped = after.get(referent).unwrap_or_else(|| {
            panic!(
                "{label}: {referent:?} ({}) missing after round-trip",
                original.class()
            )
        });

        assert_eq!(
            original.class(),
            round_tripped.class(),
            "{label}: {referent:?} class changed"
        );
        assert_eq!(
            original.name(),
            round_tripped.name(),
            "{label}: {referent:?} ({}) name changed",
            original.class()
        );
        assert_eq!(
            original.properties(),
            round_tripped.properties(),
            "{label}: {referent:?} ({} {:?}) properties changed",
            original.class(),
            original.name()
        );
        assert_eq!(
            parent_of(before, referent),
            parent_of(after, referent),
            "{label}: {referent:?} ({} {:?}) parent changed",
            original.class(),
            original.name()
        );
    }
}

#[test]
fn fps_round_trips_through_serialize() {
    let before = deserialize(FPS).unwrap();
    let bytes = serialize(&before).unwrap();
    let after = deserialize(&bytes).unwrap();

    assert_same_tree(&before, &after, "FPS");
}

#[test]
fn test_place_round_trips_through_serialize() {
    let before = deserialize(TEST_PLACE).unwrap();
    let bytes = serialize(&before).unwrap();
    let after = deserialize(&bytes).unwrap();

    assert_same_tree(&before, &after, "TestPlace");

    // Not a correctness check (Roblox's own writer orders and pads chunks
    // differently), just a data point kept for the task report.
    let scratch = Path::new(
        "/tmp/claude-1000/-mnt-data-Documents-Dev/02662147-12e3-4e0e-a3a2-acc15eb8564c/scratchpad/binary_serialize",
    );
    if scratch.is_dir() {
        std::fs::write(scratch.join("TestPlace.reserialized.rbxl"), &bytes).unwrap();
    }
    eprintln!(
        "TestPlace.rbxl: original {} bytes, re-serialized {} bytes",
        TEST_PLACE.len(),
        bytes.len()
    );
}
