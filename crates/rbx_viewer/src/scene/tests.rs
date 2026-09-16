//! Unit tests for [`super`]: DOM extraction, CFrame conversion and bounds.

use super::*;
use rbx_dom::Vector3Data;

fn test_place() -> WeakDom {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let bytes = std::fs::read(&path).expect("fixture must be readable");
    rbx_binary::deserialize(&bytes).expect("fixture must parse")
}

#[test]
fn test_place_yields_the_baseplate_and_the_spawn_but_not_the_terrain() {
    let scene = Scene::from_dom(&test_place(), &ReflectionDatabase::embedded()).unwrap();

    assert_eq!(scene.parts().len(), 2);
    // Baseplate 2048x16x2048 at y=-8 plus a 12x1x12 spawn at y=0.5: y spans -16..1.
    let center = scene.bounds().center();
    assert!(
        center.abs_diff_eq(Vec3::new(0.0, -7.5, 0.0), 1e-4),
        "{center}"
    );
    assert!((scene.bounds().radius() - 1448.5).abs() < 1.0);
}

#[test]
fn an_empty_dom_is_an_error_rather_than_an_empty_window() {
    let error = Scene::from_dom(&WeakDom::new(), &ReflectionDatabase::embedded());

    assert!(error.is_err());
}

// A quarter turn around +Y: the rotation's first column (right) must land on -Z.
// A transposed matrix would send it to +Z instead, which is exactly the mistake
// this guards against.
#[test]
fn a_non_identity_cframe_keeps_its_columns_as_basis_vectors() {
    let cframe = CFrameData {
        position: Vector3Data {
            x: 10.0,
            y: 2.0,
            z: -3.0,
        },
        rotation: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
    };

    let matrix = cframe_matrix(&cframe);

    assert!(matrix
        .transform_point3(Vec3::X)
        .abs_diff_eq(Vec3::new(10.0, 2.0, -4.0), 1e-5));
    assert!(matrix
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(10.0, 2.0, -3.0), 1e-5));
    assert!(matrix.transform_vector3(Vec3::Z).abs_diff_eq(Vec3::X, 1e-5));
}

// A part the renderer no longer draws must not offer geometry for a decal
// to be projected onto, or the decal would float where the box used to be.
#[test]
fn a_suppressed_part_offers_no_placement() {
    let mut scene = Scene::from_dom(&test_place(), &ReflectionDatabase::embedded()).unwrap();
    assert_eq!(scene.placements().len(), 2);

    scene.parts[0].suppressed = true;

    let placements = scene.placements();
    assert_eq!(placements.len(), 1);
    assert!(!placements.contains_key(&scene.parts[0].referent));
}

// ...but it is still a `BasePart` a click selects and a drag moves, so the
// outline and the gizmo that stand on its box do get one. Before they did,
// selecting any mesh in a place drew no box and no handles at all.
#[test]
fn a_suppressed_part_still_offers_a_placement_to_outline() {
    let mut scene = Scene::from_dom(&test_place(), &ReflectionDatabase::embedded()).unwrap();
    scene.parts[0].suppressed = true;

    let placements = scene.all_placements();
    assert_eq!(placements.len(), 2);
    assert_eq!(
        placements.get(&scene.parts[0].referent),
        Some(&scene.parts[0].placement())
    );
}

// An invisible part is still part of the scene's extent — the camera frames
// where the place is built, not only what happens to be painted — and a Decal on
// one still needs its placement, so only the drawing stops.
#[test]
fn a_fully_transparent_part_is_not_drawn_but_keeps_its_placement() {
    let dom = test_place();
    let mut scene = Scene::from_dom(&dom, &ReflectionDatabase::embedded()).unwrap();
    let before = scene.bounds().radius();

    scene.parts[0].alpha = 0.0;

    assert!(!scene.parts[0].is_drawn());
    assert!(scene.parts[1].is_drawn());
    assert_eq!(scene.placements().len(), 2);
    assert_eq!(scene.bounds().radius(), before);
}

#[test]
fn a_part_reads_its_transparency_and_reflectance_off_the_dom() {
    let scene = Scene::from_dom(&test_place(), &ReflectionDatabase::embedded()).unwrap();

    // TestPlace serializes neither property, which means opaque and matte.
    for part in scene.parts() {
        assert_eq!(part.alpha, 1.0);
        assert_eq!(part.reflectance, 0.0);
        assert!(part.is_drawn());
        assert!(!part.is_translucent());
    }
}

#[test]
fn a_transparency_between_zero_and_one_only_marks_a_part_translucent() {
    let mut scene = Scene::from_dom(&test_place(), &ReflectionDatabase::embedded()).unwrap();

    scene.parts[0].alpha = 0.5;

    assert!(scene.parts[0].is_drawn());
    assert!(scene.parts[0].is_translucent());
}

#[test]
fn a_missing_or_broken_float_property_reads_as_zero() {
    assert_eq!(number(None), 0.0);
    assert_eq!(number(Some(&Variant::Float32(0.25))), 0.25);
    assert_eq!(number(Some(&Variant::Float64(0.5))), 0.5);
    assert_eq!(number(Some(&Variant::Float32(f32::NAN))), 0.0);
    assert_eq!(number(Some(&Variant::Bool(true))), 0.0);
}

#[test]
fn bounds_wrap_the_corners_of_a_rotated_box() {
    // A 2x2x2 box rotated 45 degrees around Y reaches sqrt(2) along X and Z.
    let angle = std::f32::consts::FRAC_PI_4;
    let part = Part {
        material: Slot {
            layer: 0,
            kind: Kind::Plastic,
            studs_per_tile: 10.0,
        },
        kind: ShapeKind::Box,
        transform: Mat4::from_rotation_y(angle) * Mat4::from_scale(Vec3::splat(2.0)),
        color: [0.0; 3],
        alpha: 1.0,
        reflectance: 0.0,
        size: Vec3::splat(2.0),
        casts_shadow: true,
        referent: Ref::new(0),
        suppressed: false,
    };

    let bounds = bounds::of(&[part]).unwrap();

    assert!(bounds.center().abs_diff_eq(Vec3::ZERO, 1e-5));
    assert!((bounds.max.x - 2.0f32.sqrt()).abs() < 1e-5);
    assert!((bounds.max.y - 1.0).abs() < 1e-5);
}

// Studio only writes `CastShadow` when a builder has turned it off, so its
// absence has to mean the part casts — anything else would leave a whole place
// shadowless.
#[test]
fn cast_shadow_is_on_unless_a_place_says_otherwise() {
    let mut properties = std::collections::BTreeMap::new();
    assert!(casts_shadow(&properties));

    properties.insert("CastShadow".to_string(), Variant::Bool(true));
    assert!(casts_shadow(&properties));

    properties.insert("CastShadow".to_string(), Variant::Bool(false));
    assert!(!casts_shadow(&properties));
}

// A transparent part still casts in Studio (the reference capture's 0.7 slab
// has a shadow), while an invisible one has nothing to cast.
#[test]
fn only_the_invisible_and_the_opted_out_stop_casting() {
    let caster = |alpha: f32, flag: bool| Part {
        material: Slot {
            layer: 0,
            kind: Kind::Plastic,
            studs_per_tile: 10.0,
        },
        kind: ShapeKind::Box,
        transform: Mat4::IDENTITY,
        color: [0.0; 3],
        alpha,
        reflectance: 0.0,
        size: Vec3::ONE,
        casts_shadow: flag,
        referent: Ref::new(0),
        suppressed: false,
    };

    assert!(caster(1.0, true).casts_shadow());
    assert!(caster(0.3, true).casts_shadow());
    assert!(!caster(0.0, true).casts_shadow());
    assert!(!caster(1.0, false).casts_shadow());
}
