use std::rc::Rc;

use rbx_dom::{CFrameData, Vector3Data};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::properties::PropertyRow;

const WORKSPACE: Ref = Ref::new(1);
const SERVICE: Ref = Ref::new(2);

/// One instance to build: its class, what it stores, and its parent.
type Spec<'a> = (&'a str, Vec<(&'a str, Variant)>, Ref);

/// A Workspace and a MaterialService, then one instance per spec at refs
/// 10, 11, … in order.
fn place(specs: &[Spec]) -> (WeakDom, Vec<Ref>) {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(WORKSPACE, "Workspace", "Workspace"));
    dom.insert(Instance::new(SERVICE, "MaterialService", "MaterialService"));
    let mut refs = Vec::new();
    for (index, (class, values, parent)) in specs.iter().enumerate() {
        let reference = Ref::new(index as u32 + 10);
        let mut instance = Instance::new(reference, *class, "It");
        for (key, value) in values {
            instance
                .properties_mut()
                .insert((*key).to_owned(), value.clone());
        }
        dom.insert(instance);
        dom.set_parent(reference, Some(*parent));
        refs.push(reference);
    }
    (dom, refs)
}

fn rows(dom: &WeakDom, part: Ref) -> Vec<PropertyRow> {
    Properties::new(ReflectionDatabase::embedded()).rows(dom, &[part], None)
}

fn value(rows: &[PropertyRow], name: &str) -> Option<String> {
    rows.iter()
        .find(|row| row.name == name)
        .map(|row| row.value.clone())
}

fn size(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn at(x: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y: 0.0, z: 0.0 },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn custom(density: f32) -> Variant {
    Variant::PhysicalProperties(PhysicalProperties::Custom {
        density,
        friction: 0.3,
        elasticity: 0.5,
        friction_weight: 1.0,
        elasticity_weight: 1.0,
    })
}

#[test]
fn a_new_parts_mass_is_its_volume_times_plastics_density() {
    // 4 × 1.2 × 2 studs of Plastic at 0.7, per the materials table.
    let (dom, refs) = place(&[("Part", vec![], WORKSPACE)]);
    let rows = rows(&dom, refs[0]);

    assert_eq!(value(&rows, "Mass").as_deref(), Some("6.72"));
    assert_eq!(
        value(&rows, "CurrentPhysicalProperties").as_deref(),
        Some("Custom(0.7, 0.3, 0.5, 1, 1)")
    );
    assert_eq!(value(&rows, "CenterOfMass").as_deref(), Some("(0, 0, 0)"));
    assert_eq!(value(&rows, "ResizeIncrement").as_deref(), Some("1"));
    assert!(rows
        .iter()
        .all(|row| row.name != "Mass" || row.edit.is_none()));
}

#[test]
fn a_wedge_holds_half_the_mass_off_centre() {
    let (dom, refs) = place(&[("WedgePart", vec![("size", size(2.0, 3.0, 6.0))], WORKSPACE)]);
    let rows = rows(&dom, refs[0]);

    // 36 / 2 × 0.7.
    assert_eq!(value(&rows, "Mass").as_deref(), Some("12.6"));
    assert_eq!(
        value(&rows, "CenterOfMass").as_deref(),
        Some("(0, -0.5, 1)")
    );
}

#[test]
fn density_comes_from_the_part_its_material_or_its_variant() {
    let (dom, refs) = place(&[
        (
            "Part",
            vec![
                ("size", size(1.0, 1.0, 1.0)),
                ("CustomPhysicalProperties", custom(2.0)),
            ],
            WORKSPACE,
        ),
        // Enum.Material.Wood, 0.35 in the materials table.
        (
            "Part",
            vec![
                ("size", size(1.0, 1.0, 1.0)),
                ("Material", Variant::Enum(512)),
            ],
            WORKSPACE,
        ),
        (
            "Part",
            vec![
                ("size", size(1.0, 1.0, 1.0)),
                ("MaterialVariantSerialized", Variant::String("Slick".into())),
            ],
            WORKSPACE,
        ),
        (
            "MaterialVariant",
            vec![("CustomPhysicalProperties", custom(3.0))],
            SERVICE,
        ),
    ]);
    let mut dom = dom;
    dom.set_name(refs[3], "Slick").unwrap();

    assert_eq!(value(&rows(&dom, refs[0]), "Mass").as_deref(), Some("2"));
    assert_eq!(value(&rows(&dom, refs[1]), "Mass").as_deref(), Some("0.35"));
    assert_eq!(value(&rows(&dom, refs[2]), "Mass").as_deref(), Some("3"));
}

#[test]
fn what_cannot_be_worked_out_honestly_is_left_out() {
    let (dom, refs) = place(&[
        // Cylinder: an approximated collision shape.
        ("Part", vec![("shape", Variant::Enum(2))], WORKSPACE),
        // A ball with unequal sides.
        (
            "Part",
            vec![("shape", Variant::Enum(0)), ("size", size(1.0, 2.0, 3.0))],
            WORKSPACE,
        ),
        ("MeshPart", vec![], WORKSPACE),
        // A variant no MaterialService holds.
        (
            "Part",
            vec![("MaterialVariantSerialized", Variant::String("Gone".into()))],
            WORKSPACE,
        ),
    ]);

    for part in &refs {
        assert_eq!(value(&rows(&dom, *part), "Mass"), None, "{part:?}");
    }
    let ball = rows(&dom, refs[1]);
    assert_eq!(value(&ball, "CenterOfMass").as_deref(), Some("(0, 0, 0)"));
    for never in ["AssemblyLinearVelocity", "ExtentsSize", "Rotation"] {
        assert_eq!(value(&ball, never), None, "{never}");
    }
}

#[test]
fn a_ball_with_equal_sides_is_a_sphere() {
    let (dom, refs) = place(&[(
        "Part",
        vec![("shape", Variant::Enum(0)), ("size", size(2.0, 2.0, 2.0))],
        WORKSPACE,
    )]);

    // π × 2³ / 6 × 0.7.
    let mass: f32 = value(&rows(&dom, refs[0]), "Mass")
        .unwrap()
        .parse()
        .unwrap();
    assert!((mass - 2.932_153).abs() < 1e-4, "{mass}");
}

#[test]
fn a_welded_assembly_sums_its_parts_but_the_massless() {
    let (dom, refs) = place(&[
        (
            "Part",
            vec![("size", size(1.0, 1.0, 1.0)), ("CFrame", at(0.0))],
            WORKSPACE,
        ),
        (
            "Part",
            vec![("size", size(1.0, 1.0, 1.0)), ("CFrame", at(2.0))],
            WORKSPACE,
        ),
        (
            "Part",
            vec![
                ("size", size(1.0, 1.0, 1.0)),
                ("CFrame", at(9.0)),
                ("Massless", Variant::Bool(true)),
            ],
            WORKSPACE,
        ),
        (
            "WeldConstraint",
            vec![
                ("Part0Internal", Variant::Ref(Ref::new(10))),
                ("Part1Internal", Variant::Ref(Ref::new(11))),
            ],
            WORKSPACE,
        ),
        (
            "Weld",
            vec![
                ("Part0", Variant::Ref(Ref::new(11))),
                ("Part1", Variant::Ref(Ref::new(12))),
            ],
            WORKSPACE,
        ),
    ]);
    let rows = rows(&dom, refs[0]);

    assert_eq!(value(&rows, "AssemblyMass").as_deref(), Some("1.4"));
    // Weighted between the two with mass; the massless one pulls nothing.
    assert_eq!(
        value(&rows, "AssemblyCenterOfMass").as_deref(),
        Some("(1, 0, 0)")
    );
    // Two tie on RootPriority: the rule that would split them is not
    // documented, so no root is shown.
    assert_eq!(value(&rows, "AssemblyRootPart"), None);
}

#[test]
fn an_anchored_assembly_is_infinitely_heavy_and_rooted_at_its_anchor() {
    let (dom, refs) = place(&[
        (
            "Part",
            vec![("Anchored", Variant::Bool(true)), ("CFrame", at(5.0))],
            WORKSPACE,
        ),
        ("Part", vec![], WORKSPACE),
        (
            "WeldConstraint",
            vec![
                ("Part0Internal", Variant::Ref(Ref::new(10))),
                ("Part1Internal", Variant::Ref(Ref::new(11))),
            ],
            WORKSPACE,
        ),
    ]);
    let rows = rows(&dom, refs[1]);

    assert_eq!(value(&rows, "AssemblyMass").as_deref(), Some("inf"));
    assert_eq!(value(&rows, "AssemblyRootPart").as_deref(), Some("It"));
    assert_eq!(
        value(&rows, "AssemblyCenterOfMass").as_deref(),
        Some("(5, 0, 0)")
    );
}

#[test]
fn a_lone_part_is_its_own_assembly_and_a_stored_one_has_none() {
    let (dom, refs) = place(&[("Part", vec![], WORKSPACE), ("Part", vec![], SERVICE)]);

    let lone = rows(&dom, refs[0]);
    assert_eq!(value(&lone, "AssemblyMass").as_deref(), Some("6.72"));
    assert_eq!(value(&lone, "AssemblyRootPart").as_deref(), Some("It"));
    assert_eq!(value(&rows(&dom, refs[1]), "AssemblyMass"), None);
}

#[test]
fn a_weld_constraint_saved_in_an_unknown_state_leaves_the_assembly_unknown() {
    let (dom, refs) = place(&[
        ("Part", vec![], WORKSPACE),
        ("Part", vec![], WORKSPACE),
        (
            "WeldConstraint",
            vec![
                ("Part0Internal", Variant::Ref(Ref::new(10))),
                ("Part1Internal", Variant::Ref(Ref::new(11))),
                ("State", Variant::Int32(0)),
            ],
            WORKSPACE,
        ),
    ]);

    assert_eq!(value(&rows(&dom, refs[0]), "AssemblyMass"), None);
}

#[test]
fn a_move_keeps_the_joints_but_not_the_values_and_a_weld_edit_redoes_both() {
    let (mut dom, refs) = place(&[
        ("Part", vec![("CFrame", at(0.0))], WORKSPACE),
        ("Part", vec![("CFrame", at(2.0))], WORKSPACE),
        (
            "WeldConstraint",
            vec![
                ("Part0Internal", Variant::Ref(Ref::new(10))),
                ("Part1Internal", Variant::Ref(Ref::new(11))),
            ],
            WORKSPACE,
        ),
    ]);
    dom.take_changes();
    let properties = Properties::new(ReflectionDatabase::embedded());
    let center = |dom: &WeakDom| {
        value(
            &properties.rows(dom, &[refs[0]], None),
            "AssemblyCenterOfMass",
        )
    };
    let joints = || properties.joints.borrow().clone().unwrap();
    assert_eq!(center(&dom).as_deref(), Some("(1, 0, 0)"));
    let before = joints();

    dom.set_property(refs[1], "CFrame", at(4.0)).unwrap();
    properties.dom_changed(&dom.take_changes());
    assert_eq!(center(&dom).as_deref(), Some("(2, 0, 0)"));
    assert!(Rc::ptr_eq(&before, &joints()));

    dom.set_property(refs[2], "Enabled", Variant::Bool(false))
        .unwrap();
    properties.dom_changed(&dom.take_changes());
    assert_eq!(center(&dom).as_deref(), Some("(0, 0, 0)"));
    assert!(!Rc::ptr_eq(&before, &joints()));
}
