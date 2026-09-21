//! Integration tests for the live-editing mutation API: `set_property`, `set_name`,
//! `remove`, `new_instance`, and the change log drained by `take_changes`.

use rbx_dom::{Change, DomError, Instance, Ref, Variant, WeakDom};

const TEST_PLACE: &[u8] = include_bytes!("../../../assets/tests/TestPlace.rbxl");

fn dom_with_folder_and_part() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let folder = Ref::new(1);
    let part = Ref::new(2);
    dom.insert(Instance::new(folder, "Folder", "Folder"));
    dom.insert(Instance::new(part, "Part", "Part"));
    dom.set_parent(part, Some(folder));
    dom.take_changes(); // discard setup noise for the assertions below
    (dom, folder, part)
}

#[test]
fn set_property_returns_previous_value() {
    let (mut dom, _folder, part) = dom_with_folder_and_part();

    let first = dom
        .set_property(part, "Transparency", Variant::Float32(0.0))
        .unwrap();
    assert_eq!(first, None);

    let second = dom
        .set_property(part, "Transparency", Variant::Float32(0.5))
        .unwrap();
    assert_eq!(second, Some(Variant::Float32(0.0)));
    assert_eq!(
        dom.get(part).unwrap().properties().get("Transparency"),
        Some(&Variant::Float32(0.5))
    );
}

#[test]
fn set_property_unknown_referent_errors() {
    let mut dom = WeakDom::new();
    let bogus = Ref::new(999);

    let err = dom
        .set_property(bogus, "Transparency", Variant::Float32(0.0))
        .unwrap_err();
    assert_eq!(err, DomError::UnknownInstance(bogus));
}

#[test]
fn set_name_returns_previous_name_and_renames() {
    let (mut dom, _folder, part) = dom_with_folder_and_part();

    let old = dom.set_name(part, "Wall").unwrap();
    assert_eq!(old, "Part");
    assert_eq!(dom.get(part).unwrap().name(), "Wall");
}

#[test]
fn set_name_unknown_referent_errors() {
    let mut dom = WeakDom::new();
    let bogus = Ref::new(999);

    assert_eq!(
        dom.set_name(bogus, "Wall").unwrap_err(),
        DomError::UnknownInstance(bogus)
    );
}

#[test]
fn remove_deletes_subtree_and_detaches_from_parent() {
    let mut dom = WeakDom::new();
    let root = Ref::new(1);
    let branch = Ref::new(2);
    let leaf = Ref::new(3);
    dom.insert(Instance::new(root, "Folder", "Root"));
    dom.insert(Instance::new(branch, "Folder", "Branch"));
    dom.insert(Instance::new(leaf, "Part", "Leaf"));
    dom.set_parent(branch, Some(root));
    dom.set_parent(leaf, Some(branch));

    let mut removed = dom.remove(branch);
    removed.sort();
    assert_eq!(removed, vec![branch, leaf]);

    assert!(dom.get(branch).is_none());
    assert!(dom.get(leaf).is_none());
    assert!(dom.get(root).unwrap().children().is_empty());
}

#[test]
fn remove_unknown_referent_is_a_noop() {
    let mut dom = WeakDom::new();
    assert_eq!(dom.remove(Ref::new(999)), Vec::<Ref>::new());
}

#[test]
fn new_instance_is_unique_against_a_dom_loaded_from_a_real_file() {
    let mut dom = rbx_binary::deserialize(TEST_PLACE).expect("TestPlace.rbxl should parse");

    let mut existing: Vec<Ref> = dom.root_refs().to_vec();
    let mut stack = existing.clone();
    while let Some(referent) = stack.pop() {
        if let Some(instance) = dom.get(referent) {
            stack.extend_from_slice(instance.children());
            existing.extend_from_slice(instance.children());
        }
    }

    let fresh_a = dom.new_instance("Part", "Fresh A", None);
    let fresh_b = dom.new_instance("Part", "Fresh B", None);

    assert_ne!(fresh_a, fresh_b);
    assert!(!existing.contains(&fresh_a));
    assert!(!existing.contains(&fresh_b));
    assert!(dom.get(fresh_a).is_some());
    assert!(dom.get(fresh_b).is_some());
}

#[test]
fn new_instance_parents_when_requested() {
    let mut dom = WeakDom::new();
    let folder = Ref::new(1);
    dom.insert(Instance::new(folder, "Folder", "Folder"));

    let child = dom.new_instance("Part", "Child", Some(folder));

    assert_eq!(dom.get(folder).unwrap().children(), &[child]);
    assert!(!dom.root_refs().contains(&child));
}

#[test]
fn change_log_records_mutations_in_order_and_clears_on_take() {
    let mut dom = WeakDom::new();
    let folder = Ref::new(1);
    dom.insert(Instance::new(folder, "Folder", "Folder"));

    let child = dom.new_instance("Part", "Child", Some(folder));
    dom.set_property(child, "Transparency", Variant::Float32(0.0))
        .unwrap();
    dom.set_name(child, "Renamed").unwrap();
    let removed = dom.remove(child);

    let changes = dom.take_changes();
    assert_eq!(
        changes,
        vec![
            Change::Added(folder),
            Change::Added(child),
            Change::Parent {
                referent: child,
                old: None,
                new: Some(folder),
            },
            Change::Property {
                referent: child,
                name: "Transparency".to_string(),
            },
            Change::Property {
                referent: child,
                name: "Name".to_string(),
            },
            Change::Removed(child),
        ]
    );
    assert_eq!(removed, vec![child]);

    // Draining again returns nothing until something mutates the DOM again.
    assert_eq!(dom.take_changes(), Vec::new());
}

#[test]
fn set_class_changes_the_class_in_place_and_logs_it() {
    let (mut dom, folder, part) = dom_with_folder_and_part();
    dom.set_property(part, "Transparency", Variant::Float32(0.5))
        .unwrap();
    dom.new_instance("Decal", "Decal", Some(part));
    dom.take_changes();

    let old = dom.set_class(part, "WedgePart").unwrap();

    assert_eq!(old, "Part");
    let instance = dom.get(part).unwrap();
    assert_eq!(instance.class(), "WedgePart");
    // Everything but the class is the instance it was: the same referent
    // under the same parent, with its name, properties and children.
    assert_eq!(instance.referent(), part);
    assert_eq!(instance.name(), "Part");
    assert_eq!(
        instance.properties().get("Transparency"),
        Some(&Variant::Float32(0.5))
    );
    assert_eq!(instance.children().len(), 1);
    assert_eq!(dom.parent(part), Some(folder));
    assert_eq!(dom.take_changes(), vec![Change::Class(part)]);
}

#[test]
fn set_class_unknown_referent_errors_and_logs_nothing() {
    let mut dom = WeakDom::new();
    let bogus = Ref::new(999);

    let err = dom.set_class(bogus, "Folder").unwrap_err();
    assert_eq!(err, DomError::UnknownInstance(bogus));
    assert!(dom.take_changes().is_empty());
}
