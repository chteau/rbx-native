//! `MaterialVariant.AlphaMode`, under the "MaterialVariant Alpha Mode" switch.

use super::*;

/// A part drawn with a variant named `name` whose `AlphaMode` is `mode`
/// (`None` leaves the property out, as a file holding the default does).
fn variant_slot(mode: Option<u32>, alpha_modes: bool, resolved: bool) -> Slot {
    let mut properties = variant_maps();
    if let Some(mode) = mode {
        properties.push(("AlphaMode", Variant::Enum(mode)));
    }
    let dom = service_dom(&[], &[("Painted", properties)]);
    let mut catalog = Catalog::build(&dom, &ReflectionDatabase::embedded(), alpha_modes);
    let mut part = material_of("Rock");
    part.insert(
        "MaterialVariantSerialized".to_string(),
        Variant::String("Painted".to_string()),
    );
    let layer = slot(&mut catalog, &part).layer;
    if resolved {
        resolve_all(&mut catalog);
    }
    catalog.slot(layer)
}

#[test]
fn each_alpha_mode_reaches_the_slot_when_switched_on() {
    // `Enum.AlphaMode`: Overlay 0, Transparency 1, TintMask 2, Opaque 3.
    for (value, expected) in [
        (0, VariantAlpha::Overlay),
        (1, VariantAlpha::Transparency),
        (2, VariantAlpha::TintMask),
        (3, VariantAlpha::Ignored),
    ] {
        assert_eq!(variant_slot(Some(value), true, true).alpha, expected);
    }
    // Opaque is the default, so a file that leaves it out means Opaque.
    assert_eq!(variant_slot(None, true, true).alpha, VariantAlpha::Ignored);
}

#[test]
fn switched_off_every_variant_ignores_its_alpha() {
    for value in 0..=3 {
        let slot = variant_slot(Some(value), false, true);
        assert_eq!(slot.alpha, VariantAlpha::Ignored);
        assert_eq!(slot.kind, Kind::Textured);
    }
}

#[test]
fn a_colour_map_that_has_not_arrived_has_no_alpha_to_read() {
    assert_eq!(
        variant_slot(Some(1), true, false).alpha,
        VariantAlpha::Ignored
    );
}

#[test]
fn the_same_maps_under_two_modes_take_two_layers() {
    let with_mode = |mode: u32| {
        let mut properties = variant_maps();
        properties.push(("AlphaMode", Variant::Enum(mode)));
        properties
    };
    let dom = service_dom(&[], &[("Cut", with_mode(1)), ("Tinted", with_mode(2))]);
    let mut catalog = Catalog::build(&dom, &ReflectionDatabase::embedded(), true);
    let named = |name: &str| {
        let mut part = material_of("Rock");
        part.insert(
            "MaterialVariantSerialized".to_string(),
            Variant::String(name.to_string()),
        );
        part
    };

    let cut = slot(&mut catalog, &named("Cut"));
    let tinted = slot(&mut catalog, &named("Tinted"));

    assert_ne!(cut.layer, tinted.layer);
}

#[test]
fn a_builtin_material_never_carries_an_alpha_mode() {
    let mut catalog = Catalog::build(&WeakDom::new(), &ReflectionDatabase::embedded(), true);
    let wood = slot(&mut catalog, &material_of("Wood"));
    resolve_all(&mut catalog);
    assert_eq!(catalog.slot(wood.layer).alpha, VariantAlpha::Ignored);
}
