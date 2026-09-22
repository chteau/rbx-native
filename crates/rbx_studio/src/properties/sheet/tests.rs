use rbx_dom::{CFrameData, Vector3Data};

use super::*;
use crate::properties::{EditKind, PropertyRow};

const WORKSPACE: Ref = Ref::new(1);
const PART: Ref = Ref::new(2);

/// A Workspace holding one `class` named "Left" that stores `values` and
/// nothing else, the way a hand-written `.rbxlx` leaves most of a part out.
fn one(class: &str, values: &[(&str, Variant)]) -> (WeakDom, Properties) {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(WORKSPACE, "Workspace", "Workspace"));
    let mut instance = Instance::new(PART, class, "Left");
    for (name, value) in values {
        instance
            .properties_mut()
            .insert((*name).to_owned(), value.clone());
    }
    dom.insert(instance);
    dom.set_parent(PART, Some(WORKSPACE));
    (dom, Properties::new(ReflectionDatabase::embedded()))
}

fn rows(class: &str, values: &[(&str, Variant)]) -> Vec<PropertyRow> {
    let (dom, properties) = one(class, values);
    properties.rows(&dom, &[PART], None)
}

fn row<'a>(rows: &'a [PropertyRow], name: &str) -> &'a PropertyRow {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no {name} row"))
}

fn names(rows: &[PropertyRow]) -> Vec<&str> {
    rows.iter().map(|row| row.name.as_str()).collect()
}

/// What the maintainer's `align.rbxlx` stores for a part.
fn hand_written() -> Vec<(&'static str, Variant)> {
    vec![
        (
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: -12.0,
                    y: 3.0,
                    z: 0.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        ),
        (
            "size",
            Variant::Vector3(Vector3Data {
                x: 5.0,
                y: 6.0,
                z: 5.0,
            }),
        ),
        (
            "Color3uint8",
            Variant::Color3uint8 {
                r: 220,
                g: 90,
                b: 70,
            },
        ),
        ("Anchored", Variant::Bool(true)),
    ]
}

#[test]
fn a_hand_written_part_lists_studios_whole_sheet_with_defaults_filled_in() {
    let rows = rows("Part", &hand_written());

    for name in [
        "Transparency",
        "Material",
        "Reflectance",
        "CanCollide",
        "CanTouch",
        "CanQuery",
        "Locked",
        "Massless",
        "CastShadow",
        "CustomPhysicalProperties",
        "TopSurface",
        "PivotOffset",
    ] {
        row(&rows, name);
    }
    // What was stored, and what was not.
    assert_eq!(row(&rows, "Anchored").edit, Some(EditKind::Bool(true)));
    assert_eq!(row(&rows, "CanCollide").edit, Some(EditKind::Bool(true)));
    assert_eq!(row(&rows, "Locked").edit, Some(EditKind::Bool(false)));
    assert_eq!(row(&rows, "Transparency").value, "0");
    assert_eq!(row(&rows, "Material").value, "256 (Plastic)");
    // `Enum.SurfaceType.Studs`: a new part's top.
    assert_eq!(row(&rows, "TopSurface").value, "3 (Studs)");
}

#[test]
fn a_saved_spelling_shows_once_under_its_canonical_name() {
    let rows = rows("Part", &hand_written());

    assert_eq!(row(&rows, "Size").value, "(5, 6, 5)");
    assert_eq!(row(&rows, "Color").value, "(220, 90, 70)");
    assert_eq!(row(&rows, "Color").category, "Appearance");
    // Never stored here, so the class default: `Enum.PartType.Block`, and a
    // dropdown now that its enum resolves under the canonical name.
    assert!(matches!(
        &row(&rows, "Shape").edit,
        Some(EditKind::Enum { current, .. }) if current == "Block"
    ));
    for spelling in ["size", "Color3uint8", "shape"] {
        assert!(!names(&rows).contains(&spelling), "{spelling} listed");
    }
}

#[test]
fn hidden_and_deprecated_properties_stay_out_even_when_stored() {
    let rows = rows(
        "Part",
        &[
            // Hidden.
            ("BackParamA", Variant::Float32(-0.5)),
            // Deprecated, and superseded by `CollisionGroup`.
            ("CollisionGroupId", Variant::Int32(0)),
            // A saved spelling of the deprecated `FormFactor`.
            ("formFactorRaw", Variant::Enum(1)),
        ],
    );

    for name in [
        "BackParamA",
        "CollisionGroupId",
        "formFactorRaw",
        "FormFactor",
        "brickColor",
        "Position",
    ] {
        assert!(!names(&rows).contains(&name), "{name} listed");
    }
}

#[test]
fn security_and_not_scriptable_keep_nothing_out() {
    // `NotScriptable`, and set in Studio's panel per creator-docs.
    let rows = rows("Lighting", &[("Technology", Variant::Enum(3))]);

    row(&rows, "Technology");
}

#[test]
fn a_property_with_no_value_and_no_default_is_left_out() {
    let rows = rows("Part", &[]);

    for computed in [
        "AssemblyLinearVelocity",
        "AssemblyAngularVelocity",
        "ExtentsSize",
        "Rotation",
    ] {
        assert!(!names(&rows).contains(&computed), "{computed} listed");
    }
}

#[test]
fn class_name_and_parent_are_read_off_the_instance() {
    let rows = rows("Part", &[]);

    assert_eq!(row(&rows, "ClassName").value, "\"Part\"");
    assert_eq!(row(&rows, "ClassName").edit, None);
    assert_eq!(row(&rows, "Parent").value, "Workspace");
    assert_eq!(row(&rows, "Parent").edit, None);
    assert_eq!(
        row(&rows, "Name").edit,
        Some(EditKind::Text("Left".to_owned()))
    );
}

#[test]
fn a_root_instance_has_no_parent_row() {
    let (dom, properties) = one("Part", &[]);

    let rows = properties.rows(&dom, &[WORKSPACE], None);
    assert!(!names(&rows).contains(&"Parent"));
}

#[test]
fn an_unreflected_saved_spelling_still_shows_under_its_canonical_name() {
    // `Sandboxed` is newer than the bundled dump; rbx-dom records that a
    // file saves it as `DefinesCapabilities`.
    let rows = rows("Part", &[("DefinesCapabilities", Variant::Bool(true))]);

    assert_eq!(row(&rows, "Sandboxed").category, "Other");
    assert!(!names(&rows).contains(&"DefinesCapabilities"));
}

#[test]
fn each_class_is_worked_out_once() {
    let (dom, properties) = one("Part", &[]);

    let first = properties.sheet("Part");
    properties.rows(&dom, &[PART], None);
    properties.rows(&dom, &[PART], None);

    assert!(Rc::ptr_eq(&first, &properties.sheet("Part")));
    assert_eq!(properties.sheets.borrow().len(), 1);
}

#[test]
fn brick_color_is_the_closest_table_colour_to_the_parts_color() {
    let fresh = rows("Part", &[]);
    assert_eq!(row(&fresh, "BrickColor").value, "Medium stone grey");

    let red = rows(
        "Part",
        &[(
            "Color3uint8",
            Variant::Color3uint8 {
                r: 196,
                g: 40,
                b: 28,
            },
        )],
    );
    let brick = row(&red, "BrickColor");
    assert_eq!(brick.value, "Bright red");
    assert_eq!(brick.edit, Some(EditKind::BrickColor(21)));
}
