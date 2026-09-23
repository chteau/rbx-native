// The rules that decide what the diff keeps, drops and sends back:
// Keep Unknowns, creatability, Override Packages, Client priority and the
// reversal. The helpers live in the parent test module.

use rbx_dom::{Variant, WeakDom};

use super::super::*;
use super::{database, node, place, root, server_rules, with_property, Tables};

#[test]
fn an_instance_the_snapshot_does_not_know_is_removed_unless_something_keeps_it() {
    let (dom, _, part, ..) = place();
    let snapshot = root(vec![node("Workspace", "Workspace", vec![])]);

    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert_eq!(changes.removals.len(), 1);
    assert_eq!(tables.ids[&changes.removals[0]], part);

    // The setting keeps it …
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let rules = Rules {
        keep_unknowns: true,
        ..server_rules()
    };
    let changes = diff(&dom, database(), &snapshot, rules, &mut tables.as_ids());
    assert!(changes.removals.is_empty());

    // … and so does the node's own meta.
    let mut kept = root(vec![node("Workspace", "Workspace", vec![])]);
    kept.children[0].keep_unknowns = true;
    let mut tables = Tables::new();
    hydrate(&dom, &kept, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &kept,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert!(changes.removals.is_empty());
}

#[test]
fn a_service_the_snapshot_does_not_mention_is_never_removed() {
    let (dom, ..) = place();
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
    // ServerScriptService and its script are not in the snapshot, but a
    // service is not creatable, and the diff never descends into an
    // instance it didn't pair.
    assert!(changes.is_empty());
}

#[test]
fn with_override_packages_off_server_changes_under_a_package_link_are_dropped() {
    let mut dom = WeakDom::new();
    let rs = dom.new_instance("ReplicatedStorage", "ReplicatedStorage", None);
    let package = dom.new_instance("Folder", "Package", Some(rs));
    dom.new_instance("PackageLink", "PackageLink", Some(package));
    let inner = dom.new_instance("ModuleScript", "Inner", Some(package));
    let loose = dom.new_instance("ModuleScript", "Loose", Some(rs));
    let _ = (inner, loose);

    let snapshot = root(vec![node(
        "ReplicatedStorage",
        "ReplicatedStorage",
        vec![
            node(
                "Package",
                "Folder",
                vec![
                    node("PackageLink", "PackageLink", vec![]),
                    with_property(
                        node("Inner", "ModuleScript", vec![]),
                        "Source",
                        Variant::String("new".into()),
                    ),
                    node("Added", "ModuleScript", vec![]),
                ],
            ),
            with_property(
                node("Loose", "ModuleScript", vec![]),
                "Source",
                Variant::String("new".into()),
            ),
            node("AddedLoose", "ModuleScript", vec![]),
        ],
    )]);

    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let rules = Rules {
        override_packages: false,
        ..server_rules()
    };
    let changes = diff(&dom, database(), &snapshot, rules, &mut tables.as_ids());
    let added: Vec<&str> = changes.additions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(added, vec!["AddedLoose"]);
    assert_eq!(changes.updates.len(), 1);
    assert_eq!(tables.ids[&changes.updates[0].id], loose);

    // With the default (on), nothing is filtered.
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        server_rules(),
        &mut tables.as_ids(),
    );
    assert_eq!(changes.additions.len(), 2);
    assert_eq!(changes.updates.len(), 2);
}

#[test]
fn client_priority_compares_only_scripts_unless_syncback_properties_is_on() {
    let (mut dom, _, part, _, script) = place();
    dom.set_property(part, "Transparency", Variant::Float32(0.7))
        .expect("a Part has Transparency");
    dom.set_property(script, "Source", Variant::String("print(1)".into()))
        .expect("a Script has Source");
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
    let client = Rules {
        priority: Priority::Client,
        ..server_rules()
    };

    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(&dom, database(), &snapshot, client, &mut tables.as_ids());
    assert_eq!(changes.updates.len(), 1);
    assert_eq!(tables.ids[&changes.updates[0].id], script);

    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let everything = Rules {
        syncback_properties: true,
        ..client
    };
    let changes = diff(
        &dom,
        database(),
        &snapshot,
        everything,
        &mut tables.as_ids(),
    );
    assert_eq!(changes.updates.len(), 2);
}

#[test]
fn reversing_turns_additions_into_removals_updates_into_the_instances_state_and_removals_into_additions(
) {
    let (mut dom, _, part, _, script) = place();
    dom.set_property(script, "Source", Variant::String("print(1)".into()))
        .expect("a Script has Source");
    // The server has a script this DOM lacks, a stale Source for the one it
    // knows, and no Baseplate.
    let snapshot = root(vec![
        node("Workspace", "Workspace", vec![]),
        node(
            "ServerScriptService",
            "ServerScriptService",
            vec![
                with_property(
                    node("Main", "Script", vec![]),
                    "Source",
                    Variant::String("old".into()),
                ),
                node("Extra", "ModuleScript", vec![]),
            ],
        ),
    ]);
    let client = Rules {
        priority: Priority::Client,
        ..server_rules()
    };
    let mut tables = Tables::new();
    hydrate(&dom, &snapshot, &mut tables.as_ids());
    let changes = diff(&dom, database(), &snapshot, client, &mut tables.as_ids());
    assert_eq!(changes.additions.len(), 1);
    assert_eq!(changes.updates.len(), 1);
    assert_eq!(changes.removals.len(), 1);

    let reversed = reverse(&dom, &changes, &mut tables.as_ids());

    let extra_id = snapshot.children[1].children[1].id;
    assert_eq!(reversed.removals, vec![extra_id]);

    let update = &reversed.updates[0];
    assert_eq!(tables.ids[&update.id], script);
    let source = update
        .properties
        .as_ref()
        .and_then(|properties| properties.iter().find(|(name, _)| name == "Source"))
        .map(|(_, value)| argon_client::decode_value(value));
    assert_eq!(source, Some(Some(Variant::String("print(1)".into()))));

    let addition = &reversed.additions[0];
    assert_eq!(addition.name, "Baseplate");
    assert_eq!(addition.class, "Part");
    assert_eq!(tables.ids[&addition.id], part);
    assert_eq!(addition.parent, Some(snapshot.children[0].id));
}
