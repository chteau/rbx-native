use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::{commit, commit_all, edit_text};
use super::*;

fn db() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// A `Weld`, two parts and a `Folder`, all at the root.
struct Place {
    dom: WeakDom,
    weld: Ref,
    a: Ref,
    b: Ref,
    folder: Ref,
}

fn place() -> Place {
    let mut dom = WeakDom::new();
    let weld = dom.new_instance("Weld", "Weld", None);
    let a = dom.new_instance("Part", "A", None);
    let b = dom.new_instance("Part", "B", None);
    let folder = dom.new_instance("Folder", "Folder", None);
    Place {
        dom,
        weld,
        a,
        b,
        folder,
    }
}

fn part0(place: &Place, weld: Ref) -> Option<&Variant> {
    place.dom.get(weld).unwrap().properties().get("Part0")
}

#[test]
fn a_reference_round_trips_through_its_text() {
    for target in [Ref::new(7), NIL_REF] {
        let text = edit_text(&Variant::Ref(target)).unwrap();
        assert_eq!(parse_ref(&text), Ok(Variant::Ref(target)));
    }
    assert_eq!(ref_text(NIL_REF), "nil");
    assert_eq!(parse_ref(""), Ok(Variant::Ref(NIL_REF)));
    assert_eq!(parse_ref(" NIL "), Ok(Variant::Ref(NIL_REF)));
    assert!(parse_ref("Part").is_err());
}

#[test]
fn an_unset_instance_property_reads_as_nil_under_its_saved_name() {
    let place = place();
    let weld = place.dom.get(place.weld).unwrap();
    assert_eq!(
        nil_default(&db(), weld, "Part0"),
        Some(("Part0".to_owned(), Variant::Ref(NIL_REF)))
    );
    // Not instance-typed, so not a reference at all.
    assert_eq!(nil_default(&db(), weld, "Enabled"), None);
}

#[test]
fn picking_a_part_sets_an_unset_part0() {
    let mut place = place();
    let text = ref_text(place.a);

    let previous =
        commit(&mut place.dom, &db(), place.weld, "Part0", &text).expect("a part is a BasePart");

    assert_eq!(previous, None);
    assert_eq!(part0(&place, place.weld), Some(&Variant::Ref(place.a)));
}

#[test]
fn a_target_of_the_wrong_class_is_refused_and_writes_nothing() {
    let mut place = place();
    let text = ref_text(place.folder);

    let error = commit(&mut place.dom, &db(), place.weld, "Part0", &text).unwrap_err();

    assert!(error.contains("BasePart"), "{error}");
    assert_eq!(part0(&place, place.weld), None);
}

#[test]
fn a_target_that_no_longer_exists_is_refused() {
    let mut place = place();
    let result = commit(&mut place.dom, &db(), place.weld, "Part0", "999");
    assert!(result.is_err());
    assert_eq!(part0(&place, place.weld), None);
}

#[test]
fn clearing_removes_the_key_rather_than_writing_the_stand_in() {
    let mut place = place();
    commit(
        &mut place.dom,
        &db(),
        place.weld,
        "Part0",
        &ref_text(place.a),
    )
    .unwrap();

    let previous = commit(&mut place.dom, &db(), place.weld, "Part0", "nil").unwrap();

    assert_eq!(previous, Some(Variant::Ref(place.a)));
    assert_eq!(part0(&place, place.weld), None);
}

#[test]
fn an_object_value_takes_any_instance() {
    let mut place = place();
    let value = place.dom.new_instance("ObjectValue", "Target", None);

    commit(
        &mut place.dom,
        &db(),
        value,
        "Value",
        &ref_text(place.folder),
    )
    .unwrap();

    let stored = place.dom.get(value).unwrap().properties().get("Value");
    assert_eq!(stored, Some(&Variant::Ref(place.folder)));
}

#[test]
fn one_pick_sets_every_selected_weld() {
    let mut place = place();
    let second = place.dom.new_instance("Weld", "Weld2", None);
    commit(&mut place.dom, &db(), second, "Part0", &ref_text(place.b)).unwrap();

    commit_all(
        &mut place.dom,
        &db(),
        &[place.weld, second],
        "Part0",
        &ref_text(place.a),
    )
    .unwrap();

    assert_eq!(part0(&place, place.weld), Some(&Variant::Ref(place.a)));
    assert_eq!(part0(&place, second), Some(&Variant::Ref(place.a)));
}

#[test]
fn the_stand_in_is_never_a_real_instance() {
    let place = place();
    assert!(place.dom.get(NIL_REF).is_none());
    assert!(check_target(&place.dom, &db(), place.weld, "Weld", "Part0", NIL_REF).is_ok());
}

#[test]
fn a_primary_part_must_be_inside_its_model() {
    let mut place = place();
    let model = place.dom.new_instance("Model", "Model", None);
    place.dom.set_parent(place.a, Some(model));
    let check = |target| check_target(&place.dom, &db(), model, "Model", "PrimaryPart", target);

    assert!(check(place.a).is_ok());
    assert_eq!(
        check(place.b),
        Err("PrimaryPart must be a part inside this Model".to_owned())
    );
}
