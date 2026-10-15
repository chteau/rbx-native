use rbx_dom::{Instance, Ref, Variant, WeakDom};

fn build() -> (WeakDom, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let a = dom.new_instance("Folder", "A", None);
    let b = dom.new_instance("Part", "B", Some(a));
    let c = dom.new_instance("Part", "C", Some(a));
    dom.set_property(b, "Anchored", Variant::Bool(true)).unwrap();
    dom.take_changes();
    (dom, a, b, c)
}

#[test]
fn move_subtrees_round_trip_keeps_tree_and_properties() {
    let (mut dom, a, b, c) = build();
    let before = dom.clone();
    let mut other = WeakDom::new();
    assert!(dom.move_subtrees(&[a], &mut other).is_empty());
    assert!(dom.get(a).is_none());
    assert_eq!(other.root_refs(), &[a]);
    assert_eq!(other.get(a).unwrap().children(), &[b, c]);
    assert_eq!(other.parent(b), Some(a));
    assert!(other.move_subtrees(&[a], &mut dom).is_empty());
    assert_eq!(dom.root_refs(), before.root_refs());
    for r in [a, b, c] {
        assert_eq!(dom.get(r), before.get(r));
        assert_eq!(dom.parent(r), before.parent(r));
    }
}

#[test]
fn move_subtrees_logs_no_changes() {
    let (mut dom, a, _, _) = build();
    let mut other = WeakDom::new();
    dom.move_subtrees(&[a], &mut other);
    assert!(dom.take_changes().is_empty());
    assert!(other.take_changes().is_empty());
}

#[test]
fn move_subtrees_collision_left_in_place() {
    let (mut dom, a, b, _) = build();
    let mut other = WeakDom::new();
    other.insert(Instance::new(b, "Part", "Clash"));
    other.take_changes();
    let left = dom.move_subtrees(&[a], &mut other);
    assert_eq!(left, vec![a]);
    assert!(dom.get(a).is_some());
    assert_eq!(dom.root_refs(), &[a]);
    assert_eq!(other.get(b).unwrap().name(), "Clash");
    assert!(other.get(a).is_none());
}

#[test]
fn move_subtrees_raises_allocator() {
    let (mut dom, a, b, c) = build();
    let mut other = WeakDom::new();
    dom.move_subtrees(&[a], &mut other);
    let fresh = other.new_instance("Part", "New", None);
    assert!(![a, b, c].contains(&fresh));
    assert!(other.next_ref() > c.value().max(b.value()).max(a.value()));
}
