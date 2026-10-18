//! Three-way merges run through the real binary and XML formats: each input
//! is serialized and deserialized before merging, and the merged output is
//! serialized and deserialized again before it is inspected. Instances are
//! located by path, never by referent, since every round trip renumbers them.

use rbx_dom::attributes::{encode_tags, tags};
use rbx_dom::merge::{merge, Target};
use rbx_dom::{Instance, Ref, UniqueId, Variant, WeakDom};

const SOURCE_BASE: &str = "line1\nline2\nline3\n";
const SOURCE_THEIRS: &str = "line1\nline2\nchanged\n";
const SOURCE_MERGED: &str = "line1\nline2\nchanged\n";

fn uid(n: u32) -> Variant {
    Variant::UniqueId(UniqueId {
        index: n,
        time: 1,
        random: 7,
    })
}

fn add(
    dom: &mut WeakDom,
    referent: u32,
    class: &str,
    name: &str,
    parent: Option<Ref>,
    id: u32,
) -> Ref {
    let r = Ref::new(referent);
    let mut instance = Instance::new(r, class, name);
    instance.properties_mut().insert("UniqueId".into(), uid(id));
    dom.insert(instance);
    dom.set_parent(r, parent);
    r
}

fn set(dom: &mut WeakDom, r: Ref, name: &str, value: Variant) {
    dom.set_property(r, name, value).expect("set property");
}

fn named(dom: &WeakDom, r: Ref) -> &str {
    dom.get(r).map_or("", Instance::name)
}

fn find(dom: &WeakDom, path: &str) -> Ref {
    let mut names = path.split('.');
    let first = names.next().expect("non-empty path");
    let mut current = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| named(dom, *r) == first)
        .unwrap_or_else(|| panic!("no root {first} in {path}"));
    for name in names {
        current = dom
            .get(current)
            .expect("instance exists")
            .children()
            .iter()
            .copied()
            .find(|r| named(dom, *r) == name)
            .unwrap_or_else(|| panic!("no {name} in {path}"));
    }
    current
}

fn prop<'a>(dom: &'a WeakDom, path: &str, name: &str) -> Option<&'a Variant> {
    dom.get(find(dom, path))
        .and_then(|instance| instance.properties().get(name))
}

/// Workspace.Model with PartA (primary part, tagged) and PartB, plus a
/// Workspace.Script. Referents start at `base` so each side gets its own.
fn fixture(base: u32) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = add(&mut dom, base, "Workspace", "Workspace", None, 1);
    let model = add(&mut dom, base + 1, "Model", "Model", Some(workspace), 2);
    let part_a = add(&mut dom, base + 2, "Part", "PartA", Some(model), 3);
    let part_b = add(&mut dom, base + 3, "Part", "PartB", Some(model), 4);
    let script = add(&mut dom, base + 4, "Script", "Script", Some(workspace), 5);
    set(&mut dom, part_a, "Transparency", Variant::Float32(0.0));
    set(&mut dom, part_b, "Transparency", Variant::Float32(0.0));
    set(
        &mut dom,
        part_a,
        "Tags",
        Variant::String(encode_tags(&["alpha", "beta"])),
    );
    set(&mut dom, model, "PrimaryPart", Variant::Ref(part_a));
    set(
        &mut dom,
        script,
        "Source",
        Variant::String(SOURCE_BASE.into()),
    );
    dom
}

fn edited(mut dom: WeakDom, edit: impl FnOnce(&mut WeakDom)) -> WeakDom {
    edit(&mut dom);
    dom
}

fn ours_edit(dom: &mut WeakDom) {
    let part_a = find(dom, "Workspace.Model.PartA");
    dom.set_name(part_a, "Renamed").expect("rename");
}

fn theirs_edit(dom: &mut WeakDom) {
    let part_b = find(dom, "Workspace.Model.PartB");
    set(dom, part_b, "Transparency", Variant::Float32(0.5));
    let script = find(dom, "Workspace.Script");
    set(dom, script, "Source", Variant::String(SOURCE_THEIRS.into()));
    let model = find(dom, "Workspace.Model");
    let part_c = add(dom, 206, "Part", "PartC", Some(model), 6);
    set(dom, model, "PrimaryPart", Variant::Ref(part_c));
}

fn set_transparency(dom: &mut WeakDom, value: f32) {
    let part_b = find(dom, "Workspace.Model.PartB");
    set(dom, part_b, "Transparency", Variant::Float32(value));
}

fn via_binary(dom: &WeakDom) -> WeakDom {
    let bytes = rbx_binary::serialize(dom).expect("serialize binary");
    rbx_binary::deserialize(&bytes).expect("deserialize binary")
}

fn via_xml(dom: &WeakDom) -> WeakDom {
    let text = rbx_xml::serialize(dom).expect("serialize xml");
    rbx_xml::deserialize(&text).expect("deserialize xml")
}

/// Round-trips all three inputs through `format`, merges them, and returns
/// the merged output after one more trip through `format`.
fn merged_through(format: fn(&WeakDom) -> WeakDom) -> WeakDom {
    let base = format(&fixture(1));
    let ours = format(&edited(fixture(101), ours_edit));
    let theirs = format(&edited(fixture(201), theirs_edit));
    let (output, conflicts) = merge(&base, &ours, &theirs);
    assert!(conflicts.is_empty(), "unexpected conflicts: {conflicts:?}");
    format(&output)
}

fn check_merged(dom: &WeakDom) {
    assert_eq!(named(dom, find(dom, "Workspace.Model.Renamed")), "Renamed");
    assert_eq!(
        prop(dom, "Workspace.Model.PartB", "Transparency"),
        Some(&Variant::Float32(0.5))
    );
    assert_eq!(
        prop(dom, "Workspace.Script", "Source"),
        Some(&Variant::String(SOURCE_MERGED.into()))
    );
    assert_eq!(
        prop(dom, "Workspace.Model", "PrimaryPart"),
        Some(&Variant::Ref(find(dom, "Workspace.Model.PartC")))
    );
    assert_eq!(
        tags(prop(dom, "Workspace.Model.Renamed", "Tags")),
        vec!["alpha", "beta"]
    );
}

#[test]
fn binary_round_trip_merges_both_edits() {
    check_merged(&merged_through(via_binary));
}

#[test]
fn xml_round_trip_merges_both_edits() {
    check_merged(&merged_through(via_xml));
}

#[test]
fn binary_same_property_conflict_keeps_ours() {
    let base = via_binary(&fixture(1));
    let ours = via_binary(&edited(fixture(101), |d| set_transparency(d, 0.2)));
    let theirs = via_binary(&edited(fixture(201), |d| set_transparency(d, 0.8)));
    let (output, conflicts) = merge(&base, &ours, &theirs);

    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].target, Target::Property("Transparency".into()));
    let output = via_binary(&output);
    assert_eq!(
        prop(&output, "Workspace.Model.PartB", "Transparency"),
        Some(&Variant::Float32(0.2))
    );
}
