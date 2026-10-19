use super::*;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn set(dom: &mut WeakDom, referent: Ref, name: &str, on: bool) {
    dom.set_property(referent, name, Variant::Bool(on)).unwrap();
}

#[test]
fn a_selected_model_reaches_the_parts_under_it_once_each() {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "M", Some(ws));
    let a = dom.new_instance("Part", "A", Some(model));
    let nested = dom.new_instance("Model", "N", Some(model));
    let b = dom.new_instance("MeshPart", "B", Some(nested));
    let script = dom.new_instance("Script", "S", Some(model));
    let other = dom.new_instance("Part", "Other", Some(ws));

    let parts = target_parts(&dom, &database(), &[model, a]);
    assert_eq!(parts, vec![a, b]);
    assert!(!parts.contains(&other));
    assert!(target_parts(&dom, &database(), &[nested, model, ws]).contains(&other));
    assert!(target_parts(&dom, &database(), &[script]).is_empty());
}

#[test]
fn toggling_sets_all_unless_all_are_already_set() {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(ws));
    let b = dom.new_instance("Part", "B", Some(ws));
    let parts = [a, b];

    assert_eq!(state(&dom, &parts, "Anchored"), State::Off);
    assert!(toggle_target(State::Off));
    set(&mut dom, a, "Anchored", true);
    assert_eq!(state(&dom, &parts, "Anchored"), State::Mixed);
    assert!(toggle_target(State::Mixed));
    set(&mut dom, b, "Anchored", true);
    assert_eq!(state(&dom, &parts, "Anchored"), State::On);
    assert!(!toggle_target(State::On));
}

#[test]
fn a_material_write_over_many_parts_is_one_batch_and_skips_unchanged_parts() {
    let db = database();
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(ws));
    let b = dom.new_instance("Part", "B", Some(ws));
    properties::edit::commit_all(&mut dom, &db, &[b], "Material", "Neon").unwrap();
    dom.take_changes();

    properties::edit::commit_all(&mut dom, &db, &[a, b], "Material", "Neon").unwrap();
    let changes = dom.take_changes();
    assert_eq!(changes.len(), 1, "b already was Neon");
    assert!(materials(&db).iter().any(|m| m == "Neon"));
    assert!(!materials(&db).iter().any(|m| m == "Air" || m == "Water"));
}

#[test]
fn a_locked_part_is_flagged_for_the_viewport_to_skip() {
    let mut dom = WeakDom::new();
    let ws = dom.new_instance("Workspace", "Workspace", None);
    let a = dom.new_instance("Part", "A", Some(ws));
    let b = dom.new_instance("Part", "B", Some(ws));
    set(&mut dom, a, "Locked", true);
    assert!(is_locked(&dom, a));
    assert!(!is_locked(&dom, b));
}
