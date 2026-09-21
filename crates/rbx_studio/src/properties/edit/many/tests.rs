use rbx_dom::{CFrameData, Change, Vector3Data};

use super::*;
use crate::history::History;

const LEFT: Ref = Ref::new(2);
const RIGHT: Ref = Ref::new(3);

/// Two parts storing `left` and `right`, and nothing else.
fn two_parts(left: &[(&str, Variant)], right: &[(&str, Variant)]) -> WeakDom {
    let mut dom = WeakDom::new();
    for (reference, name, values) in [(LEFT, "Left", left), (RIGHT, "Right", right)] {
        let mut part = Instance::new(reference, "Part", name);
        for (key, value) in values {
            part.properties_mut()
                .insert((*key).to_owned(), value.clone());
        }
        dom.insert(part);
    }
    dom
}

fn db() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn stored<'a>(dom: &'a WeakDom, reference: Ref, key: &str) -> Option<&'a Variant> {
    dom.get(reference).unwrap().properties().get(key)
}

fn size(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

const IDENTITY: CFrameData = CFrameData {
    position: Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
};

#[test]
fn an_edit_to_a_defaulted_row_lands_under_the_name_roblox_saves() {
    let mut dom = two_parts(&[], &[]);
    let db = db();

    commit_all(&mut dom, &db, &[LEFT], "Size", "8, 1, 2").unwrap();
    commit_all(&mut dom, &db, &[LEFT], "Color", "10, 20, 30").unwrap();
    commit_all(&mut dom, &db, &[LEFT], "Shape", "Ball").unwrap();
    commit_all(&mut dom, &db, &[LEFT], "Transparency", "0.5").unwrap();

    assert_eq!(stored(&dom, LEFT, "size"), Some(&size(8.0, 1.0, 2.0)));
    assert_eq!(
        stored(&dom, LEFT, "Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 10,
            g: 20,
            b: 30
        })
    );
    // `Enum.PartType.Ball`.
    assert_eq!(stored(&dom, LEFT, "shape"), Some(&Variant::Enum(0)));
    assert_eq!(
        stored(&dom, LEFT, "Transparency"),
        Some(&Variant::Float32(0.5))
    );
    for canonical in ["Size", "Color", "Shape"] {
        assert_eq!(stored(&dom, LEFT, canonical), None, "{canonical}");
    }
}

#[test]
fn the_viewport_reads_what_a_defaulted_row_wrote() {
    let mut dom = two_parts(&[("CFrame", Variant::CFrame(IDENTITY))], &[]);
    // Nothing to draw yet: the renderer builds a part from `CFrame` and
    // `size`, and the file left `size` out.
    assert!(rbx_viewer::pick::model_of(&dom, LEFT).is_none());

    commit_all(&mut dom, &db(), &[LEFT], "Size", "8, 1, 2").unwrap();

    let model = rbx_viewer::pick::model_of(&dom, LEFT).expect("a part to draw");
    assert_eq!(model.x_axis.length(), 8.0);
}

#[test]
fn a_saved_file_carries_what_a_defaulted_row_wrote() {
    let mut dom = two_parts(&[], &[]);
    let db = db();
    commit_all(&mut dom, &db, &[LEFT], "Size", "8, 1, 2").unwrap();
    commit_all(&mut dom, &db, &[LEFT], "Color", "10, 20, 30").unwrap();

    let reloaded = rbx_xml::deserialize(&rbx_xml::serialize(&dom).unwrap()).unwrap();
    let left = reloaded
        .root_refs()
        .iter()
        .find_map(|&reference| reloaded.get(reference).filter(|part| part.name() == "Left"))
        .expect("the part");

    assert_eq!(left.properties().get("size"), Some(&size(8.0, 1.0, 2.0)));
    assert_eq!(
        left.properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 10,
            g: 20,
            b: 30
        })
    );
}

#[test]
fn an_edit_through_the_canonical_name_rewrites_the_stored_spelling() {
    let mut dom = two_parts(&[("size", size(4.0, 1.0, 2.0))], &[]);

    commit_all(&mut dom, &db(), &[LEFT], "Size", "6, 1, 2").unwrap();

    assert_eq!(stored(&dom, LEFT, "size"), Some(&size(6.0, 1.0, 2.0)));
    assert_eq!(stored(&dom, LEFT, "Size"), None);
}

#[test]
fn a_multi_edit_writes_every_instance_and_undoes_as_one_step() {
    let mut dom = two_parts(
        &[("Transparency", Variant::Float32(0.0))],
        &[("Transparency", Variant::Float32(1.0))],
    );
    let mut history = History::new(10);

    history.push(dom.clone());
    commit_all(&mut dom, &db(), &[LEFT, RIGHT], "Transparency", "0.25").unwrap();
    let changes = dom.take_changes();
    history.record_changes(changes.clone());

    for reference in [LEFT, RIGHT] {
        assert_eq!(
            stored(&dom, reference, "Transparency"),
            Some(&Variant::Float32(0.25))
        );
    }
    let written: Vec<Ref> = changes
        .iter()
        .filter_map(|change| match change {
            Change::Property { referent, .. } => Some(*referent),
            _ => None,
        })
        .collect();
    assert_eq!(written, [LEFT, RIGHT]);

    let (before, _) = history.undo(dom).expect("one step to undo");
    assert_eq!(
        stored(&before, LEFT, "Transparency"),
        Some(&Variant::Float32(0.0))
    );
    assert_eq!(
        stored(&before, RIGHT, "Transparency"),
        Some(&Variant::Float32(1.0))
    );
    assert!(history.undo(before).is_none(), "one step, not two");
}

#[test]
fn a_part_left_empty_keeps_each_instances_own_value() {
    let mut dom = two_parts(
        &[("size", size(4.0, 1.0, 2.0))],
        &[("size", size(4.0, 3.0, 2.0))],
    );

    // What the panel sends after only X was typed into a mixed row.
    commit_all(&mut dom, &db(), &[LEFT, RIGHT], "Size", "8, , 2").unwrap();

    assert_eq!(stored(&dom, LEFT, "size"), Some(&size(8.0, 1.0, 2.0)));
    assert_eq!(stored(&dom, RIGHT, "size"), Some(&size(8.0, 3.0, 2.0)));
}

#[test]
fn nothing_typed_into_a_mixed_row_changes_nothing() {
    let mut dom = two_parts(&[], &[]);
    dom.take_changes();

    commit_all(&mut dom, &db(), &[LEFT, RIGHT], "Name", "").unwrap();

    assert!(dom.take_changes().is_empty());
    assert_eq!(dom.get(LEFT).unwrap().name(), "Left");
    assert_eq!(dom.get(RIGHT).unwrap().name(), "Right");
}

#[test]
fn a_typed_name_renames_every_instance() {
    let mut dom = two_parts(&[], &[]);

    commit_all(&mut dom, &db(), &[LEFT, RIGHT], "Name", "Wall").unwrap();

    assert_eq!(dom.get(LEFT).unwrap().name(), "Wall");
    assert_eq!(dom.get(RIGHT).unwrap().name(), "Wall");
}

#[test]
fn a_value_one_instance_rejects_changes_none_of_them() {
    let mut dom = two_parts(&[], &[]);
    dom.insert(Instance::new(Ref::new(4), "Folder", "Folder"));
    dom.take_changes();

    let result = commit_all(&mut dom, &db(), &[LEFT, Ref::new(4)], "Anchored", "true");

    assert!(result.is_err());
    assert!(dom.take_changes().is_empty());
    assert_eq!(stored(&dom, LEFT, "Anchored"), None);
}

#[test]
fn a_value_already_held_is_not_written() {
    let mut dom = two_parts(&[], &[]);
    dom.take_changes();

    // The class default, re-committed by a focus leaving the field.
    commit_all(&mut dom, &db(), &[LEFT, RIGHT], "Transparency", "0").unwrap();

    assert!(dom.take_changes().is_empty());
    assert_eq!(stored(&dom, LEFT, "Transparency"), None);
}

#[test]
fn blanks_fill_part_by_part_only_for_a_value_made_of_parts() {
    assert_eq!(fill_blanks(&size(1.0, 2.0, 3.0), "9, , "), "9, 2, 3");
    assert_eq!(fill_blanks(&size(1.0, 2.0, 3.0), "  "), "1, 2, 3");
    // A string's comma is part of the string.
    let text = Variant::String("a, b".into());
    assert_eq!(fill_blanks(&text, "x, "), "x, ");
    assert_eq!(fill_blanks(&text, ""), "a, b");
}

#[test]
fn a_brick_color_edit_writes_the_parts_color() {
    let mut dom = two_parts(&[], &[]);
    let db = db();

    commit_all(&mut dom, &db, &[LEFT, RIGHT], "BrickColor", "21").unwrap();
    for part in [LEFT, RIGHT] {
        assert_eq!(
            stored(&dom, part, "Color3uint8"),
            Some(&Variant::Color3uint8 {
                r: 196,
                g: 40,
                b: 28
            })
        );
    }
    commit_all(&mut dom, &db, &[LEFT], "BrickColor", "Really red").unwrap();
    assert_eq!(
        stored(&dom, LEFT, "Color3uint8"),
        Some(&Variant::Color3uint8 { r: 255, g: 0, b: 0 })
    );
    assert!(commit_all(&mut dom, &db, &[LEFT], "BrickColor", "Nonsense").is_err());
    assert_eq!(stored(&dom, LEFT, "BrickColor"), None);
}
