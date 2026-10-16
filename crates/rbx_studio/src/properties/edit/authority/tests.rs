use rbx_dom::{Instance, Variant};

use super::*;
use crate::history::History;
use crate::properties::edit::commit_all;

fn workspace() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let reference = Ref::new(1);
    dom.insert(Instance::new(reference, CLASS, "Workspace"));
    (dom, reference)
}

fn value(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref, name: &str) -> Variant {
    let instance = dom.get(reference).unwrap();
    db.stored_or_default(instance, name).unwrap().1.clone()
}

#[test]
fn server_enables_the_five_companions_in_one_undo_step() {
    let (mut dom, workspace) = workspace();
    let db = ReflectionDatabase::embedded();
    let mut history = History::new(10);

    history.push(dom.clone());
    dom.take_changes();
    commit_all(&mut dom, &db, &[workspace], PROPERTY, "Server").unwrap();
    history.record_changes(dom.take_changes());

    assert_eq!(value(&dom, &db, workspace, PROPERTY), Variant::Enum(0));
    for (name, expected) in [
        ("NextGenerationReplication", Variant::Enum(2)),
        ("PlayerScriptsUseInputActionSystem", Variant::Enum(2)),
        ("SignalBehavior", Variant::Enum(2)),
        ("UseFixedSimulation", Variant::Enum(2)),
        ("StreamingEnabled", Variant::Bool(true)),
    ] {
        assert_eq!(value(&dom, &db, workspace, name), expected, "{name}");
    }

    let (before, _) = history.undo(dom).unwrap();
    assert_eq!(value(&before, &db, workspace, PROPERTY), Variant::Enum(1));
    assert_eq!(
        value(&before, &db, workspace, "UseFixedSimulation"),
        Variant::Enum(0)
    );
    assert_eq!(
        value(&before, &db, workspace, "StreamingEnabled"),
        Variant::Bool(false)
    );
    assert!(history.undo(before).is_none(), "one step covered all six");
}

#[test]
fn automatic_leaves_the_companions_alone() {
    let (mut dom, workspace) = workspace();
    let db = ReflectionDatabase::embedded();
    commit_all(&mut dom, &db, &[workspace], PROPERTY, "Server").unwrap();
    commit_all(&mut dom, &db, &[workspace], PROPERTY, "Automatic").unwrap();

    assert_eq!(
        value(&dom, &db, workspace, "StreamingEnabled"),
        Variant::Bool(true)
    );
    assert_eq!(
        value(&dom, &db, workspace, "SignalBehavior"),
        Variant::Enum(2)
    );
}
