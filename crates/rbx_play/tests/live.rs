use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_play::live::LiveTree;
use rbx_play::protocol::{Move, Node, Rename};
use rbx_reflection::ReflectionDatabase;

fn node(id: u64, parent: u64, class: &str, name: &str) -> Node {
    Node {
        id,
        parent,
        class: class.to_string(),
        name: name.to_string(),
    }
}

fn workspace_with_part() -> LiveTree {
    let mut tree = LiveTree::new();
    tree.add(&[
        node(1, 0, "Workspace", "Workspace"),
        node(2, 1, "Part", "Floor"),
    ]);
    tree
}

fn child_names(dom: &WeakDom, parent: Ref) -> Vec<String> {
    dom.get(parent)
        .expect("parent exists")
        .children()
        .iter()
        .map(|child| dom.get(*child).expect("child exists").name().to_string())
        .collect()
}

#[test]
fn add_builds_root_with_child() {
    let tree = workspace_with_part();

    let roots = tree.dom.root_refs();
    assert_eq!(roots.len(), 1);
    let root = tree.dom.get(roots[0]).expect("root exists");
    assert_eq!(root.class(), "Workspace");
    assert_eq!(root.children().len(), 1);
    assert_eq!(child_names(&tree.dom, roots[0]), vec!["Floor".to_string()]);

    let workspace_ref = tree.ref_of(1).expect("id 1 mapped");
    let part_ref = tree.ref_of(2).expect("id 2 mapped");
    assert_eq!(workspace_ref, roots[0]);
    assert_eq!(tree.id_of(workspace_ref), Some(1));
    assert_eq!(tree.id_of(part_ref), Some(2));
    assert_eq!(tree.dom.parent(part_ref), Some(workspace_ref));
    assert_eq!(tree.len(), 2);
    assert!(!tree.is_empty());
}

#[test]
fn child_before_parent_ends_under_parent() {
    let mut tree = workspace_with_part();
    tree.remove(&[2]);

    tree.add(&[node(3, 4, "Part", "Orphan")]);
    assert!(tree.ref_of(3).is_none());
    assert_eq!(tree.len(), 1);

    tree.add(&[node(4, 1, "Folder", "Folder")]);
    let folder = tree.ref_of(4).expect("folder mapped");
    let orphan = tree.ref_of(3).expect("orphan applied once parent arrived");
    assert_eq!(tree.dom.parent(orphan), Some(folder));
    assert_eq!(tree.dom.parent(folder), tree.ref_of(1));
    assert_eq!(child_names(&tree.dom, folder), vec!["Orphan".to_string()]);
    assert_eq!(tree.len(), 3);
}

#[test]
fn remove_drops_subtree() {
    let mut tree = workspace_with_part();
    tree.remove(&[1]);

    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());
    assert!(tree.ref_of(2).is_none());
    assert!(tree.ref_of(1).is_none());
    assert!(tree.dom.root_refs().is_empty());
}

#[test]
fn remove_unknown_id_is_ignored() {
    let mut tree = workspace_with_part();
    tree.remove(&[99]);
    assert_eq!(tree.len(), 2);
}

#[test]
fn rename_sets_name() {
    let mut tree = workspace_with_part();
    tree.rename(&[Rename {
        id: 2,
        name: "Ground".to_string(),
    }]);

    let part = tree.ref_of(2).expect("part mapped");
    assert_eq!(tree.dom.get(part).expect("part exists").name(), "Ground");
}

#[test]
fn reparent_moves_instance() {
    let mut tree = workspace_with_part();
    tree.add(&[node(5, 1, "Folder", "Folder")]);
    tree.reparent(&[Move { id: 2, parent: 5 }]);

    let part = tree.ref_of(2).expect("part mapped");
    assert_eq!(tree.dom.parent(part), tree.ref_of(5));
    assert_eq!(tree.len(), 3);
}

#[test]
fn reparent_to_zero_makes_root() {
    let mut tree = workspace_with_part();
    tree.reparent(&[Move { id: 2, parent: 0 }]);

    let part = tree.ref_of(2).expect("part mapped");
    assert!(tree.dom.root_refs().contains(&part));
    assert_eq!(tree.dom.parent(part), None);
    assert_eq!(tree.len(), 2);
}

#[test]
fn reparent_under_unknown_parent_removes() {
    let mut tree = workspace_with_part();
    tree.reparent(&[Move { id: 2, parent: 99 }]);

    assert!(tree.ref_of(2).is_none());
    assert_eq!(tree.len(), 1);
}

#[test]
fn add_existing_id_renames_instead_of_duplicating() {
    let mut tree = workspace_with_part();
    tree.add(&[node(2, 1, "Part", "Renamed")]);

    assert_eq!(tree.len(), 2);
    let roots = tree.dom.root_refs();
    assert_eq!(
        child_names(&tree.dom, roots[0]),
        vec!["Renamed".to_string()]
    );
    let part = tree.ref_of(2).expect("part mapped");
    assert_eq!(tree.dom.get(part).expect("part exists").class(), "Part");
}

#[test]
fn add_existing_id_reparents() {
    let mut tree = workspace_with_part();
    tree.add(&[node(5, 1, "Folder", "Folder")]);
    tree.add(&[node(2, 5, "Part", "Floor")]);

    let part = tree.ref_of(2).expect("part mapped");
    assert_eq!(tree.dom.parent(part), tree.ref_of(5));
    assert_eq!(tree.len(), 3);
}

#[test]
fn apply_props_decodes_values_attributes_and_tags() {
    let mut tree = workspace_with_part();
    let db = ReflectionDatabase::shared();

    let values: BTreeMap<String, serde_json::Value> = BTreeMap::from([
        (
            "Size".to_string(),
            serde_json::json!({"t": "V3", "v": [4, 1, 2]}),
        ),
        ("Anchored".to_string(), serde_json::json!(true)),
        ("Bogus".to_string(), serde_json::json!(1)),
    ]);
    let attributes: BTreeMap<String, serde_json::Value> =
        BTreeMap::from([("Speed".to_string(), serde_json::json!(5))]);
    let tags = vec!["Tag".to_string()];

    tree.apply_props(2, &values, Some(&attributes), Some(tags.as_slice()), db);

    let part = tree.ref_of(2).expect("part mapped");
    let instance = tree.dom.get(part).expect("part exists");

    match instance.properties().get("Size") {
        Some(Variant::Vector3(v)) => assert_eq!((v.x, v.y, v.z), (4.0, 1.0, 2.0)),
        other => panic!("Size not a Vector3: {other:?}"),
    }
    assert!(matches!(
        instance.properties().get("Anchored"),
        Some(Variant::Bool(true))
    ));
    assert!(instance.properties().get("Bogus").is_none());

    let attrs = instance.attributes();
    assert!(matches!(attrs.get("Speed"), Some(Variant::Float64(s)) if *s == 5.0));
    assert_eq!(instance.tags(), ["Tag"]);
}

#[test]
fn apply_props_unknown_id_is_ignored() {
    let mut tree = workspace_with_part();
    let db = ReflectionDatabase::shared();
    let values: BTreeMap<String, serde_json::Value> =
        BTreeMap::from([("Anchored".to_string(), serde_json::json!(true))]);

    tree.apply_props(99, &values, None, None, db);
    assert_eq!(tree.len(), 2);
}

#[test]
fn clear_empties_everything() {
    let mut tree = workspace_with_part();
    tree.clear();

    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());
    assert!(tree.dom.root_refs().is_empty());
    assert!(tree.ref_of(1).is_none());
    assert!(tree.ref_of(2).is_none());
}

#[test]
fn mutations_record_dom_changes() {
    let mut tree = LiveTree::new();
    tree.add(&[node(1, 0, "Workspace", "Workspace")]);
    assert!(!tree.dom.take_changes().is_empty(), "add");

    tree.add(&[node(2, 1, "Part", "Floor")]);
    assert!(!tree.dom.take_changes().is_empty(), "add child");

    tree.rename(&[Rename {
        id: 2,
        name: "Ground".to_string(),
    }]);
    assert!(!tree.dom.take_changes().is_empty(), "rename");

    tree.reparent(&[Move { id: 2, parent: 0 }]);
    assert!(!tree.dom.take_changes().is_empty(), "reparent");

    tree.remove(&[2]);
    assert!(!tree.dom.take_changes().is_empty(), "remove");

    tree.clear();
    assert!(!tree.dom.take_changes().is_empty(), "clear");
}
