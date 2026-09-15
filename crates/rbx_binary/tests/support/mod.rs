//! Fixture bytes and tree helpers shared by the integration test binaries.

// Each test binary links this module separately and uses a different subset of it,
// so unused helpers here are expected rather than dead code.
#![allow(dead_code)]

use rbx_dom::{Instance, Ref, Variant, WeakDom};

pub const FPS: &[u8] = include_bytes!("../../../../assets/tests/FPS.rbxm");
pub const TEST_PLACE: &[u8] = include_bytes!("../../../../assets/tests/TestPlace.rbxl");

// Walks the tree instead of asking the DOM for its instance count: it proves at
// the same time that every instance PRNT mentions is actually reachable.
pub fn all_refs(dom: &WeakDom) -> Vec<Ref> {
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    let mut found = Vec::new();

    while let Some(referent) = stack.pop() {
        found.push(referent);
        if let Some(instance) = dom.get(referent) {
            stack.extend_from_slice(instance.children());
        }
    }

    found
}

// Sorted by referent, which for both test files is the order the INST chunk
// declared the class in, so indices line up with the property arrays.
pub fn instances_of<'a>(dom: &'a WeakDom, class: &str) -> Vec<&'a Instance> {
    let mut refs: Vec<Ref> = all_refs(dom)
        .into_iter()
        .filter(|referent| dom.get(*referent).is_some_and(|i| i.class() == class))
        .collect();
    refs.sort();

    refs.into_iter().filter_map(|r| dom.get(r)).collect()
}

pub fn only<'a>(dom: &'a WeakDom, class: &str) -> &'a Instance {
    let instances = instances_of(dom, class);
    assert_eq!(instances.len(), 1, "expected exactly one {class}");
    instances[0]
}

pub fn property<'a>(instance: &'a Instance, name: &str) -> &'a Variant {
    instance
        .properties()
        .get(name)
        .unwrap_or_else(|| panic!("{} has no property {name}", instance.class()))
}

/// Collects one property across every instance of the tree that carries it.
pub fn every(dom: &WeakDom, name: &str) -> Vec<Variant> {
    all_refs(dom)
        .into_iter()
        .filter_map(|referent| dom.get(referent))
        .filter_map(|instance| instance.properties().get(name).cloned())
        .collect()
}

/// Finds the referent parenting `target`, if any (`None` means `target` is a root).
pub fn parent_of(dom: &WeakDom, target: Ref) -> Option<Ref> {
    all_refs(dom).into_iter().find(|&referent| {
        dom.get(referent)
            .is_some_and(|i| i.children().contains(&target))
    })
}

/// Determinant of a row-major 3×3 matrix; 1.0 for a proper rotation.
pub fn determinant(m: &[f32; 9]) -> f32 {
    m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6])
}
