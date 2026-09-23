// The initial diff is pure over a `WeakDom`, the reflection database and
// the id tables, so every rule the plugin's processor has is checked here
// against small trees: pairing by name and class, property updates both
// ways, additions, removals and what keeps them, the package filter, and
// the Client reversal.

use std::collections::HashMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;

mod rules;

pub(super) fn database() -> &'static ReflectionDatabase {
    ReflectionDatabase::shared()
}

pub(super) fn node(name: &str, class: &str, children: Vec<Snapshot>) -> Snapshot {
    Snapshot {
        id: ArgonRef::generate(),
        parent: None,
        name: name.to_owned(),
        class: class.to_owned(),
        properties: Vec::new(),
        children,
        keep_unknowns: false,
    }
}

pub(super) fn root(children: Vec<Snapshot>) -> Snapshot {
    Snapshot {
        id: ArgonRef::ROOT,
        ..node("ROOT", "DataModel", children)
    }
}

pub(super) fn with_property(mut snapshot: Snapshot, name: &str, value: Variant) -> Snapshot {
    let encoded = argon_client::encode_value(&value).expect("encodable");
    snapshot.properties.push((name.to_owned(), encoded));
    snapshot
}

pub(super) fn server_rules() -> Rules {
    Rules {
        priority: Priority::Server,
        keep_unknowns: false,
        override_packages: true,
        syncback_properties: false,
    }
}

pub(super) struct Tables {
    ids: HashMap<ArgonRef, Ref>,
    ids_rev: HashMap<Ref, ArgonRef>,
}

impl Tables {
    pub(super) fn new() -> Self {
        Tables {
            ids: HashMap::new(),
            ids_rev: HashMap::new(),
        }
    }

    pub(super) fn as_ids(&mut self) -> Ids<'_> {
        Ids {
            ids: &mut self.ids,
            ids_rev: &mut self.ids_rev,
        }
    }
}

/// A place with ServerScriptService holding one script, and Workspace
/// holding one part.
pub(super) fn place() -> (WeakDom, Ref, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Baseplate", Some(workspace));
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let script = dom.new_instance("Script", "Main", Some(sss));
    (dom, workspace, part, sss, script)
}

#[test]
fn hydrate_pairs_nodes_with_the_first_unpaired_instance_of_the_same_name_and_class() {
    let (dom, workspace, part, sss, script) = place();
    let snapshot = root(vec![
        node(
            "Workspace",
            "Workspace",
            vec![node("Baseplate", "Part", vec![])],
        ),
        node(
            "ServerScriptService",
            "ServerScriptService",
            vec![node("Main", "Script", vec![])],
        ),
    ]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());

    let ws = &snapshot.children[0];
    let sss_node = &snapshot.children[1];
    assert_eq!(tables.ids[&ws.id], workspace);
    assert_eq!(tables.ids[&ws.children[0].id], part);
    assert_eq!(tables.ids[&sss_node.id], sss);
    assert_eq!(tables.ids[&sss_node.children[0].id], script);
    assert_eq!(tables.ids_rev.len(), 4);
}

#[test]
fn two_children_of_one_name_pair_in_order_and_a_class_mismatch_does_not_pair() {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let first = dom.new_instance("Part", "Twin", Some(ws));
    let second = dom.new_instance("Part", "Twin", Some(ws));
    let snapshot = root(vec![node(
        "Workspace",
        "Workspace",
        vec![
            node("Twin", "Part", vec![]),
            node("Twin", "Part", vec![]),
            node("Twin", "Folder", vec![]),
        ],
    )]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let twins = &snapshot.children[0].children;
    assert_eq!(tables.ids[&twins[0].id], first);
    assert_eq!(tables.ids[&twins[1].id], second);
    assert!(!tables.ids.contains_key(&twins[2].id));
}

#[test]
fn a_paired_tree_with_equal_properties_produces_no_changes() {
    let (dom, ..) = place();
    let snapshot = root(vec![
        node(
            "Workspace",
            "Workspace",
            vec![node("Baseplate", "Part", vec![])],
        ),
        node(
            "ServerScriptService",
            "ServerScriptService",
            vec![node("Main", "Script", vec![])],
        ),
    ]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert!(changes.is_empty());
}

#[test]
fn an_unpaired_snapshot_node_is_an_addition_under_its_paired_parent() {
    let (dom, sss, ..) = {
        let (dom, _, _, sss, _) = place();
        (dom, sss)
    };
    let snapshot = root(vec![node(
        "ServerScriptService",
        "ServerScriptService",
        vec![
            node("Main", "Script", vec![]),
            node("Extra", "ModuleScript", vec![]),
        ],
    )]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert_eq!(changes.additions.len(), 1);
    let extra = &changes.additions[0];
    assert_eq!(extra.name, "Extra");
    assert_eq!(extra.parent, Some(snapshot.children[0].id));
    assert_eq!(tables.ids[&snapshot.children[0].id], sss);
    assert!(changes.updates.is_empty());
    assert!(changes.removals.is_empty());
}

#[test]
fn a_snapshot_property_that_differs_becomes_an_update_with_the_servers_value() {
    let (dom, ..) = place();
    let snapshot = root(vec![node(
        "Workspace",
        "Workspace",
        vec![with_property(
            node("Baseplate", "Part", vec![]),
            "Transparency",
            Variant::Float32(0.5),
        )],
    )]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert_eq!(changes.updates.len(), 1);
    let update = &changes.updates[0];
    assert_eq!(update.id, snapshot.children[0].children[0].id);
    let properties = update.properties.as_ref().expect("properties");
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].0, "Transparency");
    assert_eq!(
        argon_client::decode_value(&properties[0].1),
        Some(Variant::Float32(0.5))
    );
}

#[test]
fn a_property_the_snapshot_lacks_is_reset_to_its_class_default() {
    let (mut dom, _, part, ..) = place();
    dom.set_property(part, "Transparency", Variant::Float32(0.7))
        .expect("a Part has Transparency");
    let snapshot = root(vec![node(
        "Workspace",
        "Workspace",
        vec![node("Baseplate", "Part", vec![])],
    )]);
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    let properties = changes.updates[0].properties.as_ref().expect("properties");
    let default = database()
        .default_value("Part", "Transparency")
        .expect("a default")
        .clone();
    assert_eq!(properties.len(), 1);
    assert_eq!(argon_client::decode_value(&properties[0].1), Some(default));
}
