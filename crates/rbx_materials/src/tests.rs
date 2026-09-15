use super::*;
use rbx_reflection::ReflectionDatabase;

/// The materials Roblox publishes no texture pack for at all. Anything else
/// missing from the table is a transcription slip, not a procedural material.
/// `Plastic` is deliberately absent: it has a normal map and nothing else.
const PROCEDURAL: [&str; 5] = ["SmoothPlastic", "Neon", "ForceField", "Air", "Water"];

fn enum_material() -> Vec<String> {
    ReflectionDatabase::embedded()
        .enum_items("Material")
        .expect("the dump must describe Enum.Material")
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

#[test]
fn every_enum_material_value_has_a_row() {
    for name in enum_material() {
        assert!(
            material(&name, Set::Current).is_some(),
            "Enum.Material.{name} is missing from the current set"
        );
    }
}

#[test]
fn only_the_known_procedural_materials_lack_a_texture_pack() {
    let mut bare: Vec<String> = enum_material()
        .into_iter()
        .filter(|name| material(name, Set::Current).is_some_and(Material::is_procedural))
        .collect();
    bare.sort();

    let mut expected: Vec<String> = PROCEDURAL.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(bare, expected);
}

#[test]
fn the_table_holds_no_material_the_dump_has_never_heard_of() {
    let known = enum_material();
    for row in table::CURRENT.iter().chain(&table::LEGACY) {
        assert!(
            known.iter().any(|name| name == row.name()),
            "{} is not an Enum.Material value",
            row.name()
        );
    }
}

#[test]
fn the_legacy_set_answers_its_own_row_and_falls_back_to_the_current_one() {
    // Brick exists in both sets, with different packs.
    let legacy = material("Brick", Set::Legacy).unwrap();
    let current = material("Brick", Set::Current).unwrap();
    assert_eq!(legacy.map(MapKind::Color), Some(7546648254));
    assert_eq!(current.map(MapKind::Color), Some(9920482813));

    // Asphalt is a 2022 material: the legacy set has no row of its own for it.
    assert_eq!(
        material("Asphalt", Set::Legacy),
        material("Asphalt", Set::Current)
    );
    assert_eq!(material("Nonexistent", Set::Legacy), None);
}

#[test]
fn a_tuned_material_keeps_its_own_tiling_scale() {
    // Only the handful measured against Studio differ from the default.
    assert_eq!(
        material("Wood", Set::Current).unwrap().studs_per_tile(),
        4.0
    );
    assert_eq!(
        material("Concrete", Set::Current).unwrap().studs_per_tile(),
        12.0
    );
    assert_eq!(
        material("Marble", Set::Current).unwrap().studs_per_tile(),
        DEFAULT_STUDS_PER_TILE
    );
}

#[test]
fn only_the_six_metals_carry_a_metalness_map() {
    let metals: Vec<&str> = table::CURRENT
        .iter()
        .filter(|row| row.map(MapKind::Metalness).is_some())
        .map(Material::name)
        .collect();

    assert_eq!(
        metals,
        vec![
            "CeramicTiles",
            "CorrodedMetal",
            "DiamondPlate",
            "Foil",
            "Metal",
            "Rubber"
        ]
    );
}

#[test]
fn the_special_flavors_are_the_ones_a_renderer_cannot_texture() {
    let flavored: Vec<(&str, Flavor)> = table::CURRENT
        .iter()
        .filter(|row| row.flavor() != Flavor::Solid)
        .map(|row| (row.name(), row.flavor()))
        .collect();

    assert_eq!(
        flavored,
        vec![
            ("ForceField", Flavor::ForceField),
            ("Glass", Flavor::Glass),
            ("Neon", Flavor::Neon),
        ]
    );
    // Glass is the one flavoured material that still has a texture pack.
    assert!(!material("Glass", Set::Current).unwrap().is_procedural());
}

// Roblox ships the studs of plain plastic as a normal map with no colour map
// beside it, which is the one shape of row nothing else in the table has: the
// part's own `Color` has to survive it untouched.
#[test]
fn plastic_carries_a_normal_map_and_no_other() {
    let plastic = material("Plastic", Set::Current).unwrap();

    assert_eq!(plastic.map(MapKind::Normal), Some(9475362634));
    assert_eq!(plastic.map(MapKind::Color), None);
    assert_eq!(plastic.map(MapKind::Metalness), None);
    assert_eq!(plastic.map(MapKind::Roughness), None);
    assert_eq!(plastic.flavor(), Flavor::Solid);
    assert!(material("SmoothPlastic", Set::Current)
        .unwrap()
        .is_procedural());
}

// `index` casts the discriminant rather than searching, so a kind reordered in
// the enum but not in `ALL` would silently address the wrong map.
#[test]
fn every_kinds_index_is_where_it_sits_in_all() {
    for (position, kind) in MapKind::ALL.iter().enumerate() {
        assert_eq!(kind.index(), position);
    }
}
