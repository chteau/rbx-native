// `Shell::argon_connect`/`apply_argon_changes`/etc. need a live GPUI
// `Context<Shell>` this crate has no headless harness for — the same
// boundary `shell::history`'s own tests stop at. What's tested here is the
// pure logic underneath: parsing the dock's address field, and
// `ordered_parent_first`'s dirty-set ordering (a plain `WeakDom` needs no
// GPUI context at all).

use std::collections::HashSet;

use rbx_dom::WeakDom;

use super::*;

#[test]
fn a_plain_host_and_port_splits_on_the_colon() {
    assert_eq!(
        parse_address("localhost:8000"),
        ("localhost".to_owned(), 8000)
    );
}

#[test]
fn a_host_with_no_port_falls_back_to_argons_own_default() {
    assert_eq!(parse_address("localhost"), ("localhost".to_owned(), 8000));
}

#[test]
fn surrounding_whitespace_is_trimmed_from_both_halves() {
    assert_eq!(
        parse_address(" localhost : 8080 "),
        ("localhost".to_owned(), 8080)
    );
}

#[test]
fn a_malformed_port_falls_back_to_argons_own_default_rather_than_refusing_to_connect() {
    assert_eq!(
        parse_address("localhost:not-a-port"),
        ("localhost".to_owned(), 8000)
    );
}

fn index_of(ordered: &[Ref], referent: Ref) -> usize {
    ordered
        .iter()
        .position(|&r| r == referent)
        .expect("referent must be in the ordered list")
}

#[test]
fn a_parent_and_child_both_dirty_place_the_parent_first() {
    // Exactly the shape a package install dirties in one batch: a Folder
    // plus a child ModuleScript, both new in the same edit.
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "Packages", None);
    let script = dom.new_instance("ModuleScript", "net", Some(folder));
    let dirty: HashSet<Ref> = [folder, script].into_iter().collect();

    let ordered = ordered_parent_first(&dom, dirty);

    assert!(
        index_of(&ordered, folder) < index_of(&ordered, script),
        "the parent must be assigned an ArgonRef before its child looks it up"
    );
}

#[test]
fn a_three_level_batch_orders_grandparent_before_parent_before_child() {
    let mut dom = WeakDom::new();
    let root = dom.new_instance("Folder", "Packages", None);
    let index = dom.new_instance("Folder", "_Index", Some(root));
    let leaf = dom.new_instance("ModuleScript", "net", Some(index));
    let dirty: HashSet<Ref> = [root, index, leaf].into_iter().collect();

    let ordered = ordered_parent_first(&dom, dirty);

    assert!(index_of(&ordered, root) < index_of(&ordered, index));
    assert!(index_of(&ordered, index) < index_of(&ordered, leaf));
}

#[test]
fn a_parent_outside_the_batch_does_not_block_its_child() {
    // The common case: only one new instance under an already-synced,
    // already-known parent (nothing unusual for the ordering to resolve).
    let mut dom = WeakDom::new();
    let existing_parent = dom.new_instance("Folder", "Packages", None);
    let child = dom.new_instance("ModuleScript", "net", Some(existing_parent));
    let dirty: HashSet<Ref> = [child].into_iter().collect();

    let ordered = ordered_parent_first(&dom, dirty);

    assert_eq!(ordered, vec![child]);
}

#[test]
fn root_level_referents_need_no_parent_to_be_ready() {
    let mut dom = WeakDom::new();
    let a = dom.new_instance("Folder", "A", None);
    let b = dom.new_instance("Folder", "B", None);
    let dirty: HashSet<Ref> = [a, b].into_iter().collect();

    let ordered = ordered_parent_first(&dom, dirty);

    assert_eq!(ordered.len(), 2);
}
