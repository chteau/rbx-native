//! Per-instance content merging through the public `merge` API: properties,
//! names, classes, `Ref` properties, tags, attributes and script sources.

use rbx_dom::attributes;
use rbx_dom::merge::{merge, Conflict, Merge, Resolution, ResolveError, Target};
use rbx_dom::{Instance, Ref, UniqueId, Variant, WeakDom};

fn uid(n: u32) -> Variant {
    Variant::UniqueId(UniqueId {
        index: n,
        time: 1,
        random: 7,
    })
}

fn add(dom: &mut WeakDom, r: u32, class: &str, name: &str, parent: Option<u32>, id: u32) -> Ref {
    let r = Ref::new(r);
    let mut i = Instance::new(r, class, name);
    i.properties_mut().insert("UniqueId".into(), uid(id));
    dom.insert(i);
    if let Some(p) = parent {
        dom.set_parent(r, Some(Ref::new(p)));
    }
    r
}

/// Workspace > Model > leaf, with referents offset from `offset` so each
/// side's copy of an instance has a different referent.
fn scene(offset: u32, leaf: &str) -> WeakDom {
    let mut dom = WeakDom::new();
    add(&mut dom, offset + 1, "Workspace", "Workspace", None, 1);
    add(&mut dom, offset + 2, "Model", "Model", Some(offset + 1), 2);
    add(&mut dom, offset + 3, leaf, "Part", Some(offset + 2), 3);
    dom
}

fn set(dom: &mut WeakDom, r: u32, name: &str, v: Variant) {
    dom.set_property(Ref::new(r), name, v)
        .expect("set property");
}

fn find<'a>(dom: &'a WeakDom, path: &str) -> Option<&'a Instance> {
    let mut level: &[Ref] = dom.root_refs();
    let mut found = None;
    for name in path.split('.') {
        let inst = level
            .iter()
            .filter_map(|r| dom.get(*r))
            .find(|i| i.name() == name)?;
        level = inst.children();
        found = Some(inst);
    }
    found
}

fn prop(dom: &WeakDom, path: &str, name: &str) -> Option<Variant> {
    find(dom, path).and_then(|i| i.properties().get(name).cloned())
}

fn referent_of(dom: &WeakDom, path: &str) -> Ref {
    find(dom, path).expect("instance present").referent()
}

fn attrs(pairs: &[(&str, &str)]) -> Variant {
    let map: std::collections::BTreeMap<String, Variant> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Variant::String(v.to_string())))
        .collect();
    let bytes = attributes::encode(&map, |_| None).expect("attributes encode");
    Variant::String(String::from_utf8(bytes).expect("attribute blob is utf-8"))
}

fn transparency_merge() -> Merge {
    let mut base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut base, 3, "Transparency", Variant::Float32(0.0));
    set(&mut ours, 103, "Transparency", Variant::Float32(0.2));
    set(&mut theirs, 203, "Transparency", Variant::Float32(0.4));
    Merge::new(base, ours, theirs)
}

fn only_conflict(conflicts: &[Conflict]) -> &Conflict {
    assert_eq!(conflicts.len(), 1, "expected exactly one conflict");
    &conflicts[0]
}

#[test]
fn property_changed_only_by_theirs_takes_theirs() {
    let base = scene(0, "Part");
    let ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut theirs, 203, "Transparency", Variant::Float32(0.4));
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.4))
    );
}

#[test]
fn property_changed_equally_by_both_is_no_conflict() {
    let mut base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut base, 3, "Transparency", Variant::Float32(0.0));
    set(&mut ours, 103, "Transparency", Variant::Float32(0.7));
    set(&mut theirs, 203, "Transparency", Variant::Float32(0.7));
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.7))
    );
}

#[test]
fn property_changed_differently_is_a_conflict_and_keeps_ours() {
    let m = transparency_merge();
    let c = only_conflict(m.conflicts());
    assert_eq!(c.target, Target::Property("Transparency".into()));
    assert_eq!(c.base, Some(Variant::Float32(0.0)));
    assert_eq!(c.ours, Some(Variant::Float32(0.2)));
    assert_eq!(c.theirs, Some(Variant::Float32(0.4)));
    assert_eq!(m.unresolved(), 1);
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.2))
    );
}

#[test]
fn resolving_a_property_conflict_to_theirs_takes_theirs() {
    let mut m = transparency_merge();
    m.resolve(0, Resolution::Theirs).expect("resolve");
    assert_eq!(m.unresolved(), 0);
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.4))
    );
}

#[test]
fn resolving_a_property_conflict_to_base_takes_base() {
    let mut m = transparency_merge();
    m.resolve(0, Resolution::Base).expect("resolve");
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.0))
    );
}

#[test]
fn resolving_a_property_conflict_with_custom_value_uses_it() {
    let mut m = transparency_merge();
    m.resolve(0, Resolution::Custom(Variant::Float32(0.9)))
        .expect("resolve");
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Transparency"),
        Some(Variant::Float32(0.9))
    );
}

#[test]
fn property_removed_by_theirs_and_untouched_by_ours_is_absent() {
    let mut base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut base, 3, "Transparency", Variant::Float32(0.5));
    set(&mut ours, 103, "Transparency", Variant::Float32(0.5));
    theirs
        .remove_property(Ref::new(203), "Transparency")
        .expect("remove property");
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(prop(&out, "Workspace.Model.Part", "Transparency"), None);
}

#[test]
fn rename_by_theirs_only_takes_theirs_name() {
    let base = scene(0, "Part");
    let ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    theirs.set_name(Ref::new(203), "Gamma").expect("rename");
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert!(find(&out, "Workspace.Model.Gamma").is_some());
    assert!(find(&out, "Workspace.Model.Part").is_none());
}

#[test]
fn renames_on_both_sides_conflict_on_name() {
    let base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    ours.set_name(Ref::new(103), "Alpha").expect("rename");
    theirs.set_name(Ref::new(203), "Beta").expect("rename");
    let mut m = Merge::new(base, ours, theirs);
    {
        let c = only_conflict(m.conflicts());
        assert_eq!(c.target, Target::Name);
        assert_eq!(c.base, Some(Variant::String("Part".into())));
        assert_eq!(c.ours, Some(Variant::String("Alpha".into())));
        assert_eq!(c.theirs, Some(Variant::String("Beta".into())));
    }
    assert!(find(&m.output(), "Workspace.Model.Alpha").is_some());
    assert_eq!(
        m.resolve(0, Resolution::Custom(Variant::Bool(true))),
        Err(ResolveError::Unsupported(0))
    );
    m.resolve(0, Resolution::Custom(Variant::String("Delta".into())))
        .expect("resolve");
    assert!(find(&m.output(), "Workspace.Model.Delta").is_some());
}

#[test]
fn class_changed_differently_is_a_class_conflict() {
    let base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    ours.set_class(Ref::new(103), "MeshPart").expect("class");
    theirs.set_class(Ref::new(203), "WedgePart").expect("class");
    let (_, conflicts) = merge(&base, &ours, &theirs);
    let c = only_conflict(&conflicts);
    assert_eq!(c.target, Target::Class);
    assert_eq!(c.ours, Some(Variant::String("MeshPart".into())));
    assert_eq!(c.theirs, Some(Variant::String("WedgePart".into())));
}

#[test]
fn ref_property_set_by_theirs_names_output_referent() {
    let base = scene(0, "Part");
    let ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut theirs, 202, "PrimaryPart", Variant::Ref(Ref::new(203)));
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model", "PrimaryPart"),
        Some(Variant::Ref(referent_of(&out, "Workspace.Model.Part")))
    );
}

#[test]
fn ref_to_instance_theirs_added_resolves_to_added_instance() {
    let base = scene(0, "Part");
    let ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    add(&mut theirs, 250, "Part", "Part2", Some(202), 9);
    set(&mut theirs, 202, "PrimaryPart", Variant::Ref(Ref::new(250)));
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model", "PrimaryPart"),
        Some(Variant::Ref(referent_of(&out, "Workspace.Model.Part2")))
    );
}

#[test]
fn refs_to_same_target_with_different_referents_do_not_conflict() {
    let base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut ours, 102, "PrimaryPart", Variant::Ref(Ref::new(103)));
    set(&mut theirs, 202, "PrimaryPart", Variant::Ref(Ref::new(203)));
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model", "PrimaryPart"),
        Some(Variant::Ref(referent_of(&out, "Workspace.Model.Part")))
    );
}

#[test]
fn tags_merge_as_a_set_without_conflict() {
    let mut base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(
        &mut base,
        3,
        "Tags",
        Variant::String(attributes::encode_tags(&["base"])),
    );
    set(
        &mut ours,
        103,
        "Tags",
        Variant::String(attributes::encode_tags(&["base", "a"])),
    );
    set(
        &mut theirs,
        203,
        "Tags",
        Variant::String(attributes::encode_tags(&["b"])),
    );
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    let tags_value = prop(&out, "Workspace.Model.Part", "Tags").expect("tags present");
    let mut tags = attributes::tags(Some(&tags_value));
    tags.sort();
    assert_eq!(tags, ["a", "b"]);
}

#[test]
fn attributes_set_on_different_names_are_both_kept() {
    let base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut ours, 103, "AttributesSerialize", attrs(&[("X", "1")]));
    set(
        &mut theirs,
        203,
        "AttributesSerialize",
        attrs(&[("Y", "2")]),
    );
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    let value = prop(&out, "Workspace.Model.Part", "AttributesSerialize");
    let decoded = attributes::decode(value.as_ref());
    assert_eq!(decoded.get("X"), Some(&Variant::String("1".into())));
    assert_eq!(decoded.get("Y"), Some(&Variant::String("2".into())));
}

#[test]
fn same_attribute_set_differently_is_an_attribute_conflict() {
    let base = scene(0, "Part");
    let mut ours = scene(100, "Part");
    let mut theirs = scene(200, "Part");
    set(&mut ours, 103, "AttributesSerialize", attrs(&[("X", "1")]));
    set(
        &mut theirs,
        203,
        "AttributesSerialize",
        attrs(&[("X", "2")]),
    );
    let (_, conflicts) = merge(&base, &ours, &theirs);
    let c = only_conflict(&conflicts);
    assert_eq!(c.target, Target::Attribute("X".into()));
    assert_eq!(c.ours, Some(Variant::String("1".into())));
    assert_eq!(c.theirs, Some(Variant::String("2".into())));
}

#[test]
fn script_source_edits_on_separate_lines_merge_cleanly() {
    let mut base = scene(0, "Script");
    let mut ours = scene(100, "Script");
    let mut theirs = scene(200, "Script");
    set(
        &mut base,
        3,
        "Source",
        Variant::String("a\nb\nc\nd\ne\n".into()),
    );
    set(
        &mut ours,
        103,
        "Source",
        Variant::String("A\nb\nc\nd\ne\n".into()),
    );
    set(
        &mut theirs,
        203,
        "Source",
        Variant::String("a\nb\nc\nd\nE\n".into()),
    );
    let (out, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty());
    assert_eq!(
        prop(&out, "Workspace.Model.Part", "Source"),
        Some(Variant::String("A\nb\nc\nd\nE\n".into()))
    );
}

#[test]
fn overlapping_source_edits_conflict_and_keep_ours_hunk() {
    let mut base = scene(0, "Script");
    let mut ours = scene(100, "Script");
    let mut theirs = scene(200, "Script");
    set(&mut base, 3, "Source", Variant::String("a\nb\nc\n".into()));
    set(
        &mut ours,
        103,
        "Source",
        Variant::String("a\nB1\nc\n".into()),
    );
    set(
        &mut theirs,
        203,
        "Source",
        Variant::String("a\nB2\nc\n".into()),
    );
    let mut m = Merge::new(base, ours, theirs);
    {
        let c = only_conflict(m.conflicts());
        assert_eq!(c.target, Target::Source(0));
        assert!(matches!(&c.ours, Some(Variant::String(s)) if s.contains("B1")));
        assert!(matches!(&c.theirs, Some(Variant::String(s)) if s.contains("B2")));
    }
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Source"),
        Some(Variant::String("a\nB1\nc\n".into()))
    );
    m.resolve(0, Resolution::Custom(Variant::String("X\n".into())))
        .expect("resolve");
    assert_eq!(
        prop(&m.output(), "Workspace.Model.Part", "Source"),
        Some(Variant::String("a\nX\nc\n".into()))
    );
}
