use rbx_dom::Vector3Data;
use rbx_reflection::ReflectionDatabase;

use super::*;

const WORKSPACE: Ref = Ref::new(1);

/// One instance to put in a [`place`]: its class, its name, and what it
/// stores.
type Spec<'a> = (&'a str, &'a str, Vec<(&'a str, Variant)>);

/// A Workspace holding one instance per spec, at refs 2, 3, … in order.
fn place(instances: &[Spec]) -> (WeakDom, Properties, Vec<Ref>) {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(WORKSPACE, "Workspace", "Workspace"));
    let mut refs = Vec::new();
    for (index, (class, name, values)) in instances.iter().enumerate() {
        let reference = Ref::new(index as u32 + 2);
        let mut instance = Instance::new(reference, *class, *name);
        for (key, value) in values {
            instance
                .properties_mut()
                .insert((*key).to_owned(), value.clone());
        }
        dom.insert(instance);
        dom.set_parent(reference, Some(WORKSPACE));
        refs.push(reference);
    }
    (dom, Properties::new(ReflectionDatabase::embedded()), refs)
}

fn rows_of(instances: &[Spec]) -> Vec<PropertyRow> {
    let (dom, properties, refs) = place(instances);
    properties.rows(&dom, &refs, None)
}

fn row<'a>(rows: &'a [PropertyRow], name: &str) -> &'a PropertyRow {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no {name} row"))
}

fn has(rows: &[PropertyRow], name: &str) -> bool {
    rows.iter().any(|row| row.name == name)
}

fn size(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

#[test]
fn two_parts_share_every_row_one_part_has() {
    let one = rows_of(&[("Part", "Left", vec![])]);
    let two = rows_of(&[("Part", "Left", vec![]), ("Part", "Right", vec![])]);

    let names = |rows: &[PropertyRow]| rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&two), names(&one));
}

#[test]
fn a_shared_value_shows_as_usual() {
    let rows = rows_of(&[
        (
            "Part",
            "Left",
            vec![("Transparency", Variant::Float32(0.5))],
        ),
        (
            "Part",
            "Right",
            vec![("Transparency", Variant::Float32(0.5))],
        ),
    ]);

    let transparency = row(&rows, "Transparency");
    assert_eq!(transparency.value, "0.5");
    assert!(!transparency.mixed);
    // Defaulted on both, so shared too.
    assert_eq!(row(&rows, "CanCollide").edit, Some(EditKind::Bool(true)));
}

#[test]
fn a_differing_value_shows_the_mixed_state() {
    let rows = rows_of(&[
        (
            "Part",
            "Left",
            vec![
                ("Anchored", Variant::Bool(true)),
                ("Material", Variant::Enum(256)),
            ],
        ),
        ("Part", "Right", vec![("Material", Variant::Enum(528))]),
    ]);

    let anchored = row(&rows, "Anchored");
    assert!(anchored.mixed);
    assert!(anchored.value.is_empty());
    assert!(matches!(anchored.edit, Some(EditKind::Bool(_))));

    let name = row(&rows, "Name");
    assert!(name.mixed);
    assert_eq!(name.edit, Some(EditKind::Text(String::new())));

    let material = row(&rows, "Material");
    assert!(matches!(
        &material.edit,
        Some(EditKind::Enum { current, items }) if current.is_empty() && !items.is_empty()
    ));
}

#[test]
fn a_mixed_vector_keeps_the_parts_its_values_share() {
    let rows = rows_of(&[
        ("Part", "Left", vec![("size", size(4.0, 1.0, 2.0))]),
        ("Part", "Right", vec![("size", size(4.0, 3.0, 2.0))]),
    ]);

    let size = row(&rows, "Size");
    assert!(size.mixed);
    assert!(matches!(
        &size.edit,
        Some(EditKind::Fields { values, .. }) if values == &["4", "", "2"]
    ));
}

#[test]
fn a_mixed_flag_set_is_read_only_until_it_agrees() {
    let rows = rows_of(&[
        (
            "Part",
            "Left",
            vec![(
                "CustomPhysicalProperties",
                Variant::PhysicalProperties(rbx_dom::PhysicalProperties::Default),
            )],
        ),
        (
            "Part",
            "Right",
            vec![(
                "CustomPhysicalProperties",
                Variant::PhysicalProperties(edit::DEFAULT_PHYSICAL),
            )],
        ),
    ]);

    assert_eq!(row(&rows, "CustomPhysicalProperties").edit, None);
}

#[test]
fn different_classes_keep_only_what_they_share() {
    let part_and_wedge = rows_of(&[("Part", "A", vec![]), ("WedgePart", "B", vec![])]);
    // Every `BasePart` property, but not the `Part`'s own `Shape`.
    assert!(has(&part_and_wedge, "Anchored"));
    assert!(has(&part_and_wedge, "Size"));
    assert!(!has(&part_and_wedge, "Shape"));

    let part_and_folder = rows_of(&[("Part", "A", vec![]), ("Folder", "B", vec![])]);
    // Only what every `Instance` has.
    assert!(has(&part_and_folder, "Name"));
    assert!(has(&part_and_folder, "ClassName"));
    assert!(row(&part_and_folder, "ClassName").mixed);
    assert!(!has(&part_and_folder, "Anchored"));
    // A lone folder's colour row is not one of them.
    assert!(!has(&part_and_folder, edit::FOLDER_COLOR_PROPERTY));
}

#[test]
fn two_properties_sharing_a_name_are_not_the_same_property() {
    // A part's `Size` is a `Vector3`, a frame's a `UDim2`: nothing in common.
    let rows = rows_of(&[
        ("Part", "A", vec![("size", size(1.0, 1.0, 1.0))]),
        ("Frame", "B", vec![]),
    ]);

    assert!(!has(&rows, "Size"));
}

#[test]
fn the_title_names_the_nearest_shared_class_and_the_count() {
    let (dom, properties, refs) = place(&[
        ("Part", "A", vec![]),
        ("MeshPart", "B", vec![]),
        ("Part", "C", vec![]),
        ("Folder", "D", vec![]),
    ]);

    let title = |selection: &[Ref]| properties.title(&dom, selection);
    assert_eq!(title(&refs[..1]).as_deref(), Some("Part \"A\""));
    assert_eq!(title(&[refs[0], refs[2]]).as_deref(), Some("Part (2)"));
    assert_eq!(title(&refs[..3]).as_deref(), Some("BasePart (3)"));
    assert_eq!(title(&refs).as_deref(), Some("Instance (4)"));
    assert_eq!(title(&[]), None);
}

#[test]
fn shared_parts_blank_only_what_differs() {
    let values = [
        &size(1.0, 2.0, 3.0),
        &size(1.0, 5.0, 3.0),
        &size(1.0, 2.0, 4.0),
    ];
    let seeds = vec!["1".to_owned(), "2".to_owned(), "3".to_owned()];

    assert_eq!(shared_parts(seeds, &values), ["1", "", ""]);
}
