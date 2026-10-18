//! Tree-level merge: which instances survive, where they hang, and in what
//! order. Each side uses its own referents (base 1.., ours 101.., theirs
//! 201..) so that matching is proven to go by `UniqueId`, not by `Ref`.

use std::collections::HashSet;

use rbx_dom::merge::{merge, Merge, Resolution, ResolveError, Side, Target};
use rbx_dom::{Instance, Ref, UniqueId, Variant, WeakDom};

fn uid(n: u32) -> Variant {
    Variant::UniqueId(UniqueId {
        index: n,
        time: 1,
        random: 7,
    })
}

fn add(
    dom: &mut WeakDom,
    r: u32,
    class: &str,
    name: &str,
    parent: Option<u32>,
    id: Option<u32>,
) -> Ref {
    let r = Ref::new(r);
    let mut inst = Instance::new(r, class, name);
    if let Some(id) = id {
        inst.properties_mut().insert("UniqueId".into(), uid(id));
    }
    dom.insert(inst);
    if let Some(p) = parent {
        dom.set_parent(r, Some(Ref::new(p)));
    }
    r
}

fn ws(dom: &mut WeakDom, r: u32) {
    add(dom, r, "Workspace", "Workspace", None, Some(1));
}

fn set_prop(dom: &mut WeakDom, r: u32, name: &str, v: Variant) {
    dom.set_property(Ref::new(r), name, v)
        .expect("instance exists");
}

fn set_anchored(dom: &mut WeakDom, r: u32, anchored: bool) {
    set_prop(dom, r, "Anchored", Variant::Bool(anchored));
}

fn named(dom: &WeakDom, refs: &[Ref], name: &str) -> Option<Ref> {
    refs.iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.name() == name))
}

fn find(dom: &WeakDom, path: &str) -> Option<Ref> {
    let mut parts = path.split('.');
    let mut cur = named(dom, dom.root_refs(), parts.next()?)?;
    for part in parts {
        cur = named(dom, dom.get(cur)?.children(), part)?;
    }
    Some(cur)
}

fn named_all(dom: &WeakDom, parent: &str, name: &str) -> Vec<Ref> {
    let Some(p) = find(dom, parent) else {
        return Vec::new();
    };
    let Some(inst) = dom.get(p) else {
        return Vec::new();
    };
    inst.children()
        .iter()
        .copied()
        .filter(|r| dom.get(*r).is_some_and(|i| i.name() == name))
        .collect()
}

fn child_names(dom: &WeakDom, parent: &str) -> Vec<String> {
    let Some(p) = find(dom, parent) else {
        return Vec::new();
    };
    let Some(inst) = dom.get(p) else {
        return Vec::new();
    };
    inst.children()
        .iter()
        .filter_map(|r| dom.get(*r))
        .map(|i| i.name().to_owned())
        .collect()
}

fn anchored_of(dom: &WeakDom, r: Ref) -> Option<Variant> {
    dom.get(r)?.properties().get("Anchored").cloned()
}

fn anchored(dom: &WeakDom, path: &str) -> Option<Variant> {
    anchored_of(dom, find(dom, path)?)
}

/// The set of referents reachable from the roots, or `None` when one is
/// reached twice (a shared child or a cycle).
fn reachable(dom: &WeakDom) -> Option<HashSet<Ref>> {
    let mut seen = HashSet::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        if !seen.insert(r) {
            return None;
        }
        stack.extend_from_slice(dom.get(r)?.children());
    }
    Some(seen)
}

/// Workspace at `off`, folders F1 (`off+1`) and F2 (`off+2`), and a Part
/// (`off+3`, uid 112) under F1 or F2.
fn folders(dom: &mut WeakDom, off: u32, part_in_f2: bool) {
    ws(dom, off);
    add(dom, off + 1, "Folder", "F1", Some(off), Some(110));
    add(dom, off + 2, "Folder", "F2", Some(off), Some(111));
    let parent = if part_in_f2 { off + 2 } else { off + 1 };
    add(dom, off + 3, "Part", "Part", Some(parent), Some(112));
}

#[test]
fn theirs_added_part_keeps_properties_and_children() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "Part", Some(201), Some(50));
    set_anchored(&mut theirs, 202, true);
    add(&mut theirs, 203, "Folder", "Child", Some(202), Some(51));

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(anchored(&out, "Workspace.Part"), Some(Variant::Bool(true)));
    assert!(find(&out, "Workspace.Part.Child").is_some());
}

#[test]
fn ours_and_theirs_adds_with_different_ids_both_survive() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "Part", Some(101), Some(60));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "Part", Some(201), Some(70));

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert_eq!(named_all(&out, "Workspace", "Part").len(), 2);
    assert!(conflicts.is_empty());
}

#[test]
fn same_id_added_on_both_sides_is_one_instance() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "Part", Some(101), Some(80));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "Part", Some(201), Some(80));

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert_eq!(named_all(&out, "Workspace", "Part").len(), 1);
    assert!(conflicts.is_empty());
}

#[test]
fn theirs_removes_untouched_instance() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "Part", Some(1), Some(90));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "Part", Some(101), Some(90));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(find(&out, "Workspace.Part").is_none());
    assert!(conflicts.is_empty());
}

#[test]
fn ours_removes_untouched_instance() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "Part", Some(1), Some(91));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "Part", Some(201), Some(91));

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(find(&out, "Workspace.Part").is_none());
    assert!(conflicts.is_empty());
}

#[test]
fn theirs_removing_a_part_ours_modified_is_a_conflict() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "Part", Some(1), Some(90));
    set_anchored(&mut base, 2, false);
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "Part", Some(101), Some(90));
    set_anchored(&mut ours, 102, true);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);

    let mut m = Merge::new(base, ours, theirs);
    assert_eq!(m.conflicts().len(), 1);
    assert_eq!(
        m.conflicts()[0].target,
        Target::Existence {
            removed_by: Side::Theirs
        }
    );
    assert!(find(&m.output(), "Workspace.Part").is_some());
    assert_eq!(m.unresolved(), 1);

    assert_eq!(
        m.resolve(0, Resolution::Base),
        Err(ResolveError::Unsupported(0))
    );
    assert_eq!(m.unresolved(), 1);

    m.resolve(0, Resolution::Theirs)
        .expect("Theirs is supported");
    assert_eq!(m.unresolved(), 0);
    assert!(find(&m.output(), "Workspace.Part").is_none());

    m.unresolve(0);
    assert_eq!(m.unresolved(), 1);
}

#[test]
fn ours_removing_model_with_modified_child_conflicts_once_at_the_model() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Model", "Model", Some(1), Some(100));
    add(&mut base, 3, "Part", "Part", Some(2), Some(101));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Model", "Model", Some(201), Some(100));
    add(&mut theirs, 203, "Part", "Part", Some(202), Some(101));
    set_anchored(&mut theirs, 203, true);

    let mut m = Merge::new(base, ours, theirs);
    assert_eq!(m.conflicts().len(), 1);
    assert_eq!(m.conflicts()[0].name, "Model");
    assert_eq!(
        m.conflicts()[0].target,
        Target::Existence {
            removed_by: Side::Ours
        }
    );
    assert!(find(&m.output(), "Workspace.Model").is_none());

    m.resolve(0, Resolution::Theirs)
        .expect("Theirs is supported");
    let out = m.output();
    assert!(find(&out, "Workspace.Model").is_some());
    assert_eq!(
        anchored(&out, "Workspace.Model.Part"),
        Some(Variant::Bool(true))
    );
}

#[test]
fn theirs_reparent_moves_the_part() {
    let mut base = WeakDom::new();
    folders(&mut base, 1, false);
    let mut ours = WeakDom::new();
    folders(&mut ours, 101, false);
    let mut theirs = WeakDom::new();
    folders(&mut theirs, 201, true);

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(find(&out, "Workspace.F2.Part").is_some());
    assert!(find(&out, "Workspace.F1.Part").is_none());
    assert!(conflicts.is_empty());
}

#[test]
fn both_reparent_to_different_folders_conflicts_and_resolves_to_theirs() {
    let mut base = WeakDom::new();
    folders(&mut base, 1, false);
    let mut ours = WeakDom::new();
    folders(&mut ours, 101, true);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Folder", "F1", Some(201), Some(110));
    add(&mut theirs, 203, "Folder", "F2", Some(201), Some(111));
    add(&mut theirs, 205, "Folder", "F3", Some(201), Some(113));
    add(&mut theirs, 204, "Part", "Part", Some(205), Some(112));

    let mut m = Merge::new(base, ours, theirs);
    assert_eq!(m.conflicts().len(), 1);
    assert_eq!(m.conflicts()[0].target, Target::Parent);
    assert!(find(&m.output(), "Workspace.F2.Part").is_some());

    m.resolve(0, Resolution::Theirs)
        .expect("Theirs is supported");
    let out = m.output();
    assert!(find(&out, "Workspace.F3.Part").is_some());
    assert!(find(&out, "Workspace.F2.Part").is_none());
}

#[test]
fn crossed_moves_do_not_create_a_cycle() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Folder", "A", Some(1), Some(120));
    add(&mut base, 3, "Folder", "B", Some(1), Some(121));

    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Folder", "A", Some(101), Some(120));
    add(&mut ours, 103, "Folder", "B", Some(101), Some(121));
    ours.set_parent(Ref::new(102), Some(Ref::new(103)));

    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Folder", "A", Some(201), Some(120));
    add(&mut theirs, 203, "Folder", "B", Some(201), Some(121));
    theirs.set_parent(Ref::new(203), Some(Ref::new(202)));

    let mut m = Merge::new(base, ours, theirs);
    assert!(m.conflicts().iter().any(|c| c.target == Target::Parent));
    let out = m.output();
    let seen = reachable(&out).expect("output is a tree");
    assert!(seen.contains(&Ref::new(102)));
    assert!(seen.contains(&Ref::new(103)));
    m.resolve(0, Resolution::Ours).expect("Ours is supported");
    assert!(reachable(&m.output()).is_some());
}

#[test]
fn theirs_inserted_child_lands_between_its_neighbours() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "P1", Some(1), Some(130));
    add(&mut base, 3, "Part", "P2", Some(1), Some(131));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "P1", Some(101), Some(130));
    add(&mut ours, 103, "Part", "P2", Some(101), Some(131));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "P1", Some(201), Some(130));
    add(&mut theirs, 203, "Folder", "New", Some(201), Some(132));
    add(&mut theirs, 204, "Part", "P2", Some(201), Some(131));

    let (out, _) = merge(&base, &ours, &theirs);
    assert_eq!(child_names(&out, "Workspace"), ["P1", "New", "P2"]);
}

#[test]
fn ours_order_of_existing_children_is_kept() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "P1", Some(1), Some(130));
    add(&mut base, 3, "Part", "P2", Some(1), Some(131));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 103, "Part", "P2", Some(101), Some(131));
    add(&mut ours, 102, "Part", "P1", Some(101), Some(130));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "P1", Some(201), Some(130));
    add(&mut theirs, 204, "Part", "P2", Some(201), Some(131));

    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert_eq!(child_names(&out, "Workspace"), ["P2", "P1"]);
    assert!(conflicts.is_empty());
}

#[test]
fn matching_without_unique_ids_is_by_position() {
    let mut base = WeakDom::new();
    add(&mut base, 1, "Workspace", "Workspace", None, None);
    add(&mut base, 2, "Part", "Part", Some(1), None);
    add(&mut base, 3, "Part", "Part", Some(1), None);
    set_anchored(&mut base, 2, false);
    set_anchored(&mut base, 3, false);
    let mut ours = WeakDom::new();
    add(&mut ours, 101, "Workspace", "Workspace", None, None);
    add(&mut ours, 102, "Part", "Part", Some(101), None);
    add(&mut ours, 103, "Part", "Part", Some(101), None);
    set_anchored(&mut ours, 102, false);
    set_anchored(&mut ours, 103, false);
    let mut theirs = WeakDom::new();
    add(&mut theirs, 201, "Workspace", "Workspace", None, None);
    add(&mut theirs, 202, "Part", "Part", Some(201), None);
    add(&mut theirs, 203, "Part", "Part", Some(201), None);
    set_anchored(&mut theirs, 202, false);
    set_anchored(&mut theirs, 203, true);

    let (out, conflicts) = merge(&base, &ours, &theirs);
    let parts = named_all(&out, "Workspace", "Part");
    assert_eq!(parts.len(), 2);
    assert_eq!(anchored_of(&out, parts[0]), Some(Variant::Bool(false)));
    assert_eq!(anchored_of(&out, parts[1]), Some(Variant::Bool(true)));
    assert!(conflicts.is_empty());
}

#[test]
fn theirs_added_instance_referent_is_stable_across_resolutions() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    add(&mut base, 2, "Part", "X", Some(1), Some(140));
    set_prop(&mut base, 2, "Transparency", Variant::Float32(0.0));
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    add(&mut ours, 102, "Part", "X", Some(101), Some(140));
    set_prop(&mut ours, 102, "Transparency", Variant::Float32(0.5));
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);
    add(&mut theirs, 202, "Part", "X", Some(201), Some(140));
    set_prop(&mut theirs, 202, "Transparency", Variant::Float32(0.9));
    add(&mut theirs, 203, "Part", "Added", Some(201), Some(141));

    let mut m = Merge::new(base, ours, theirs);
    let before = find(&m.output(), "Workspace.Added").expect("added instance is in output");
    let id = m
        .conflicts()
        .iter()
        .position(|c| c.target == Target::Property("Transparency".into()))
        .expect("transparency conflict");
    m.resolve(id, Resolution::Theirs)
        .expect("Theirs is supported");
    let after = find(&m.output(), "Workspace.Added").expect("added instance is in output");

    assert_eq!(before, after);
    assert!(before.value() >= m.ours().next_ref());
}

#[test]
fn resolve_rejects_unknown_conflict_and_unsupported_base() {
    let mut base = WeakDom::new();
    ws(&mut base, 1);
    let mut ours = WeakDom::new();
    ws(&mut ours, 101);
    let mut theirs = WeakDom::new();
    ws(&mut theirs, 201);

    let mut m = Merge::new(base, ours, theirs);
    assert_eq!(
        m.resolve(99, Resolution::Ours),
        Err(ResolveError::UnknownConflict(99))
    );
    assert_eq!(m.unresolved(), 0);
}
