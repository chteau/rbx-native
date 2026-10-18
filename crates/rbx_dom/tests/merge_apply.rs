use rbx_dom::merge::{apply, Merge};
use rbx_dom::{Change, Instance, Ref, Variant, WeakDom};

/// Workspace(1) holds A(2, Anchored), Model M(3) with B(4), C(5, Anchored),
/// D(6) and Folder E(7).
fn build() -> WeakDom {
    let mut dom = WeakDom::new();
    add(&mut dom, 1, "Workspace", "Workspace", None, &[]);
    add(
        &mut dom,
        2,
        "Part",
        "A",
        Some(1),
        &[("Anchored", Variant::Bool(true))],
    );
    add(&mut dom, 3, "Model", "M", Some(1), &[]);
    add(&mut dom, 4, "Part", "B", Some(3), &[]);
    add(
        &mut dom,
        5,
        "Part",
        "C",
        Some(1),
        &[("Anchored", Variant::Bool(true))],
    );
    add(&mut dom, 6, "Part", "D", Some(1), &[]);
    add(&mut dom, 7, "Folder", "E", Some(1), &[]);
    dom
}

fn add(
    dom: &mut WeakDom,
    referent: u32,
    class: &str,
    name: &str,
    parent: Option<u32>,
    props: &[(&str, Variant)],
) -> Ref {
    let r = Ref::new(referent);
    let mut instance = Instance::new(r, class, name);
    for (key, value) in props {
        instance
            .properties_mut()
            .insert((*key).to_owned(), value.clone());
    }
    dom.insert(instance);
    if let Some(p) = parent {
        dom.set_parent(r, Some(Ref::new(p)));
    }
    r
}

/// One line per instance in depth-first child order: class, name, referent
/// and every property, indented by depth. Equal snapshots mean equal trees.
fn snapshot(dom: &WeakDom) -> Vec<String> {
    let mut out = Vec::new();
    for &root in dom.root_refs() {
        walk(dom, root, 0, &mut out);
    }
    out
}

fn walk(dom: &WeakDom, r: Ref, depth: usize, out: &mut Vec<String>) {
    let Some(instance) = dom.get(r) else {
        return;
    };
    out.push(format!(
        "{}{} {} #{} {:?}",
        "  ".repeat(depth),
        instance.class(),
        instance.name(),
        r.value(),
        instance.properties()
    ));
    for &child in instance.children() {
        walk(dom, child, depth + 1, out);
    }
}

fn child_named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

#[test]
fn applies_every_kind_of_edit() {
    let from = build();
    let mut target = from.clone();
    let mut to = from.clone();
    to.set_name(Ref::new(2), "Renamed").unwrap();
    to.set_class(Ref::new(7), "Model").unwrap();
    to.set_property(Ref::new(5), "Anchored", Variant::Bool(false))
        .unwrap();
    to.remove_property(Ref::new(2), "Anchored").unwrap();
    let new = to.new_instance("Part", "New", Some(Ref::new(1)));
    to.set_property(Ref::new(5), "Target", Variant::Ref(new))
        .unwrap();
    to.remove(Ref::new(6));
    to.set_parent(Ref::new(5), Some(Ref::new(3)));
    to.place_after(Ref::new(2), new);

    target.take_changes();
    apply(&mut target, &from, &to);

    assert_eq!(snapshot(&target), snapshot(&to));
    let changes = target.take_changes();
    assert!(changes
        .iter()
        .any(|c| matches!(c, Change::Added(r) if *r == new)));
    assert!(changes
        .iter()
        .any(|c| matches!(c, Change::Removed(r) if *r == Ref::new(6))));
}

#[test]
fn identical_output_logs_no_changes() {
    let from = build();
    let mut target = from.clone();
    let to = from.clone();

    target.take_changes();
    apply(&mut target, &from, &to);

    assert_eq!(snapshot(&target), snapshot(&from));
    assert!(target.take_changes().is_empty());
}

#[test]
fn inserted_instance_avoids_referent_taken_by_user_edit() {
    let from = build();
    let mut target = from.clone();
    let mut to = from.clone();
    let new = to.new_instance("Part", "Ins", Some(Ref::new(1)));
    to.set_property(Ref::new(5), "Target", Variant::Ref(new))
        .unwrap();

    let user = target.new_instance("Folder", "User", Some(Ref::new(1)));
    assert_eq!(user, new);

    target.take_changes();
    apply(&mut target, &from, &to);

    let ins = child_named(&target, Ref::new(1), "Ins").expect("Ins inserted under Workspace");
    assert_ne!(ins, user);
    assert_eq!(target.get(ins).unwrap().class(), "Part");
    assert_eq!(target.get(user).unwrap().name(), "User");
    assert_eq!(target.get(user).unwrap().class(), "Folder");
    assert_eq!(
        target.get(Ref::new(5)).unwrap().properties().get("Target"),
        Some(&Variant::Ref(ins))
    );
}

#[test]
fn edits_to_instance_removed_from_target_are_skipped() {
    let from = build();
    let mut target = from.clone();
    let mut to = from.clone();
    to.set_name(Ref::new(2), "X").unwrap();
    to.set_property(Ref::new(5), "Anchored", Variant::Bool(false))
        .unwrap();

    target.remove(Ref::new(2));
    target.take_changes();
    apply(&mut target, &from, &to);

    assert!(target.get(Ref::new(2)).is_none());
    assert_eq!(
        target
            .get(Ref::new(5))
            .unwrap()
            .properties()
            .get("Anchored"),
        Some(&Variant::Bool(false))
    );
}

#[test]
fn merge_output_is_laid_over_live_ours() {
    let base = build();
    let mut ours = base.clone();
    ours.set_name(Ref::new(5), "OursC").unwrap();
    let mut theirs = base.clone();
    theirs
        .set_property(Ref::new(5), "Anchored", Variant::Bool(false))
        .unwrap();
    theirs.new_instance("Part", "T", Some(Ref::new(1)));

    let merge = Merge::new(base, ours.clone(), theirs);
    let output = merge.output();

    let mut live = ours.clone();
    live.take_changes();
    apply(&mut live, merge.ours(), &output);

    assert_eq!(snapshot(&live), snapshot(&output));
}
