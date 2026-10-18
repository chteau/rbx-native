//! The decidable half of the gradient rows: that every shape and tile the
//! inspector offers is an item the dump knows, at the ordinal the GUI
//! renderer draws it as, and that picking one writes that ordinal. Drawing
//! the buttons needs a live window (see `shell::group`'s own tests).

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::actions::enum_item;
use super::{Choice, GRADIENT, SHAPE, TILE};
use crate::properties::edit::commit;

fn gradient() -> (WeakDom, Ref, ReflectionDatabase) {
    let mut dom = WeakDom::new();
    let frame = dom.new_instance("Frame", "Frame", None);
    let gradient = dom.new_instance(GRADIENT, GRADIENT, Some(frame));
    (dom, gradient, ReflectionDatabase::embedded())
}

/// Each offered item's ordinal in the dump.
fn ordinals(database: &ReflectionDatabase, choice: &Choice) -> Vec<u32> {
    let items = database
        .enum_items(choice.enumeration)
        .unwrap_or_else(|| panic!("the dump has no {}", choice.enumeration));
    choice
        .items
        .iter()
        .map(|(name, ..)| {
            items
                .iter()
                .find(|(item, _)| item == name)
                .unwrap_or_else(|| panic!("{} has no {name}", choice.enumeration))
                .1
        })
        .collect()
}

#[test]
fn every_offered_item_is_the_dump_s_at_the_ordinal_the_renderer_draws() {
    let database = ReflectionDatabase::embedded();
    // `rbx_viewer`'s `plan::gradient` reads `Type` 1 as radial and 2 as
    // conical, `TileMode` 1 as repeat and 2 as mirror; anything else falls
    // back to linear and clamp.
    assert_eq!(ordinals(&database, &SHAPE), [0, 1, 2]);
    assert_eq!(ordinals(&database, &TILE), [0, 1, 2]);
}

#[test]
fn only_elliptical_is_left_out_of_the_shapes() {
    let database = ReflectionDatabase::embedded();
    let offered: Vec<&str> = SHAPE.items.iter().map(|(name, ..)| *name).collect();
    let missing: Vec<&str> = database
        .enum_items(SHAPE.enumeration)
        .unwrap()
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| !offered.contains(name))
        .collect();
    assert_eq!(missing, ["Elliptical"]);
    let tiles = database.enum_items(TILE.enumeration).unwrap();
    assert_eq!(tiles.len(), TILE.items.len());
}

#[test]
fn a_fresh_gradient_reads_as_linear_and_clamped() {
    let (dom, gradient, database) = gradient();
    let read = |choice: &Choice| {
        enum_item(
            &dom,
            &database,
            gradient,
            choice.property,
            choice.enumeration,
        )
    };
    assert_eq!(read(&SHAPE), Some("Linear"));
    assert_eq!(read(&TILE), Some("Clamp"));
}

#[test]
fn picking_an_item_writes_its_ordinal_and_reads_back_by_name() {
    let (mut dom, gradient, database) = gradient();
    for choice in [&SHAPE, &TILE] {
        for (ordinal, (name, ..)) in choice.items.iter().enumerate() {
            commit(&mut dom, &database, gradient, choice.property, name).unwrap();
            let stored = dom.get(gradient).unwrap().properties().get(choice.property);
            assert_eq!(stored, Some(&Variant::Enum(ordinal as u32)));
            assert_eq!(
                enum_item(
                    &dom,
                    &database,
                    gradient,
                    choice.property,
                    choice.enumeration
                ),
                Some(*name)
            );
        }
    }
}

#[test]
fn the_scale_field_writes_a_float_the_renderer_reads() {
    let (mut dom, gradient, database) = gradient();
    commit(&mut dom, &database, gradient, "Scale", "0.5").unwrap();
    let stored = dom.get(gradient).unwrap().properties().get("Scale");
    assert_eq!(stored, Some(&Variant::Float32(0.5)));
}
