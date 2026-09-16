use super::*;
use rbx_dom::{Instance, Ref};

/// `Enum.Material` values, which the tests name rather than spell out.
fn value(name: &str) -> u32 {
    ReflectionDatabase::embedded()
        .enum_items(MATERIAL_ENUM)
        .expect("the dump must describe Enum.Material")
        .iter()
        .find(|(item, _)| item == name)
        .map(|(_, value)| *value)
        .expect("the test names a real material")
}

fn part_with(properties: &[(&str, Variant)]) -> BTreeMap<String, Variant> {
    properties
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect()
}

fn material_of(name: &str) -> BTreeMap<String, Variant> {
    part_with(&[(MATERIAL_ENUM, Variant::Enum(value(name)))])
}

/// A DOM whose `MaterialService` carries `properties` and the given variants.
fn service_dom(
    properties: &[(&str, Variant)],
    variants: &[(&str, Vec<(&str, Variant)>)],
) -> WeakDom {
    let mut dom = WeakDom::new();
    let service = Ref::new(1);
    let mut instance = Instance::new(service, MATERIAL_SERVICE, MATERIAL_SERVICE);
    for (name, value) in properties {
        instance
            .properties_mut()
            .insert(name.to_string(), value.clone());
    }
    dom.insert(instance);
    dom.set_parent(service, None);

    for (index, (name, properties)) in variants.iter().enumerate() {
        let child = Ref::new(index as u32 + 2);
        let mut instance = Instance::new(child, MATERIAL_VARIANT, *name);
        for (property, value) in properties {
            instance
                .properties_mut()
                .insert(property.to_string(), value.clone());
        }
        dom.insert(instance);
        dom.set_parent(child, Some(service));
    }

    dom
}

fn built(dom: &WeakDom) -> Catalog {
    Catalog::new(dom, &ReflectionDatabase::embedded())
}

fn slot(catalog: &mut Catalog, properties: &BTreeMap<String, Variant>) -> Slot {
    catalog.slot_for(properties, &ReflectionDatabase::embedded())
}

#[test]
fn a_part_with_no_material_property_is_plastic_with_its_own_normal_map() {
    let mut catalog = built(&WeakDom::new());

    let slot = slot(&mut catalog, &part_with(&[]));

    assert_eq!(slot.kind, Kind::Plastic);
    // Layer 0 is the mapless fallback every unresolved part points at; Plastic
    // has a normal map of its own, so it takes the layer beside it.
    assert_eq!(slot.layer, 1);
    assert_eq!(catalog.layers(), 2);
    assert_eq!(catalog.asset_refs(), vec![AssetRef::Id(9475362634)]);
}

// Shaded as plastic is not the same as carrying no maps: `Plastic` is drawn
// from the part's own `Color` like the three below and still samples the one
// normal map Roblox publishes for it.
#[test]
fn the_materials_with_no_pack_at_all_share_the_plastic_layer() {
    let mut catalog = built(&WeakDom::new());

    for name in ["SmoothPlastic", "Air", "Water"] {
        let slot = slot(&mut catalog, &material_of(name));
        assert_eq!(slot.kind, Kind::Plastic, "{name}");
        assert_eq!(slot.layer, 0, "{name}");
    }

    let plastic = slot(&mut catalog, &material_of("Plastic"));
    assert_eq!(plastic.kind, Kind::Plastic);
    assert_ne!(plastic.layer, 0);
}

#[test]
fn a_textured_material_takes_a_layer_of_its_own_with_its_tiling_scale() {
    let mut catalog = built(&WeakDom::new());

    let wood = slot(&mut catalog, &material_of("Wood"));
    let brick = slot(&mut catalog, &material_of("Brick"));
    // The same material twice is the same layer, whatever else the part says.
    let again = slot(&mut catalog, &material_of("Wood"));

    assert_eq!(wood.kind, Kind::Textured);
    assert_eq!(wood.studs_per_tile, 4.0);
    assert_eq!(brick.studs_per_tile, 5.0);
    assert_ne!(wood.layer, brick.layer);
    assert_eq!(again, wood);
    assert_eq!(catalog.layers(), 3);
    // Colour, normal and roughness for each; neither has a metalness map.
    assert_eq!(catalog.asset_refs().len(), 6);
}

#[test]
fn the_special_flavors_reach_the_shader_as_their_own_kinds() {
    let mut catalog = built(&WeakDom::new());

    assert_eq!(slot(&mut catalog, &material_of("Neon")).kind, Kind::Neon);
    assert_eq!(
        slot(&mut catalog, &material_of("ForceField")).kind,
        Kind::ForceField
    );
    assert_eq!(slot(&mut catalog, &material_of("Glass")).kind, Kind::Glass);
}

#[test]
fn use_2022_materials_false_selects_the_legacy_pack() {
    let legacy = service_dom(&[("Use2022MaterialsXml", Variant::Bool(false))], &[]);
    let mut catalog = built(&legacy);
    let mut current = built(&WeakDom::new());

    let slot = slot(&mut catalog, &material_of("Brick"));
    let modern = slot_ref(&mut current, "Brick");

    assert_eq!(catalog.asset_refs()[0], AssetRef::Id(7546648254));
    assert_eq!(modern[0], AssetRef::Id(9920482813));
    assert_eq!(slot.kind, Kind::Textured);
}

fn slot_ref(catalog: &mut Catalog, name: &str) -> Vec<AssetRef> {
    slot(catalog, &material_of(name));
    catalog.asset_refs()
}

/// The maps of the one variant both override tests use.
fn variant_maps() -> Vec<(&'static str, Variant)> {
    vec![
        ("BaseMaterial", Variant::Enum(value("Rock"))),
        (
            "ColorMap",
            Variant::String("rbxassetid://17697791225".to_string()),
        ),
        (
            "NormalMap",
            Variant::String("rbxassetid://17697791233".to_string()),
        ),
        ("StudsPerTile", Variant::Float32(9.1)),
    ]
}

#[test]
fn a_part_naming_a_variant_is_drawn_with_it() {
    let dom = service_dom(&[], &[("HyperRealisticGrayRock 1", variant_maps())]);
    let mut catalog = built(&dom);

    let mut properties = material_of("Rock");
    properties.insert(
        "MaterialVariantSerialized".to_string(),
        Variant::String("HyperRealisticGrayRock 1".to_string()),
    );
    let slot = slot(&mut catalog, &properties);

    assert_eq!(slot.kind, Kind::Textured);
    assert_eq!(slot.studs_per_tile, 9.1);
    assert_eq!(
        catalog.asset_refs(),
        vec![AssetRef::Id(17697791225), AssetRef::Id(17697791233)]
    );
}

#[test]
fn a_service_wide_substitution_replaces_the_builtin_material() {
    let dom = service_dom(
        &[(
            "RockName",
            Variant::String("HyperRealisticGrayRock 1".into()),
        )],
        &[("HyperRealisticGrayRock 1", variant_maps())],
    );
    let mut catalog = built(&dom);

    // The part names no variant at all: the service is what redirects it.
    let rock = slot(&mut catalog, &material_of("Rock"));
    // Another material is left on its own built-in pack.
    let slate = slot(&mut catalog, &material_of("Slate"));

    assert_eq!(rock.studs_per_tile, 9.1);
    assert_ne!(rock.layer, slate.layer);
    assert!(catalog.asset_refs().contains(&AssetRef::Id(17697791225)));
    assert!(catalog.asset_refs().contains(&AssetRef::Id(9920599782)));
}

// `WoodName = "Wood"` is the default value of that property, not a redirection.
#[test]
fn a_name_property_equal_to_its_own_material_overrides_nothing() {
    let dom = service_dom(
        &[("WoodName", Variant::String("Wood".to_string()))],
        &[("Wood", vec![("BaseMaterial", Variant::Enum(value("Rock")))])],
    );
    let mut catalog = built(&dom);

    slot(&mut catalog, &material_of("Wood"));

    assert_eq!(catalog.asset_refs()[0], AssetRef::Id(9920625290));
}

#[test]
fn a_layer_whose_pack_never_downloaded_falls_back_to_plastic() {
    let mut catalog = built(&WeakDom::new());
    let wood = slot(&mut catalog, &material_of("Wood"));
    assert_eq!(wood.kind, Kind::Textured);

    catalog.resolve(HashMap::new());

    assert_eq!(catalog.slot(wood.layer).kind, Kind::Plastic);
    // Neon needs no download and keeps glowing.
    let mut lit = built(&WeakDom::new());
    let neon = slot(&mut lit, &material_of("Neon"));
    lit.resolve(HashMap::new());
    assert_eq!(lit.slot(neon.layer).kind, Kind::Neon);
}

#[test]
fn a_resolved_layer_hands_its_images_out_by_map_kind() {
    let mut catalog = built(&WeakDom::new());
    let wood = slot(&mut catalog, &material_of("Wood"));
    let image = Image {
        width: 1,
        height: 1,
        pixels: vec![1, 2, 3, 4],
    };

    let color = catalog.asset_refs()[0].clone();
    catalog.resolve(HashMap::from([(color, Arc::new(image.clone()))]));

    let layer = wood.layer as usize;
    assert_eq!(catalog.image(layer, MapKind::Color), Some(&image));
    // Downloaded nothing else, and wood has no metalness map to begin with.
    assert_eq!(catalog.image(layer, MapKind::Normal), None);
    assert_eq!(catalog.image(layer, MapKind::Metalness), None);
    assert_eq!(catalog.slot(wood.layer).kind, Kind::Textured);
}

// What a scene rebuild compares before keeping the texture arrays it already
// uploaded: the same materials, resolved to the same downloads, must read
// back identical — layer for layer, map for map.
#[test]
fn resolved_maps_agree_between_two_catalogs_of_the_same_materials() {
    let build = || {
        let mut catalog = built(&WeakDom::new());
        slot(&mut catalog, &material_of("Wood"));
        slot(&mut catalog, &material_of("Neon"));
        let color = catalog.asset_refs()[0].clone();
        catalog.resolve(HashMap::from([(
            color,
            Arc::new(Image {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            }),
        )]));
        catalog
    };

    assert_eq!(build().resolved_maps(), build().resolved_maps());
}

// A map that never downloaded is uploaded as the neutral fill, exactly as a
// layer without that map is — so the key says `None` for both, and a catalog
// whose download failed does not read as different from one that never had
// the map.
#[test]
fn resolved_maps_reads_an_undownloaded_map_as_absent() {
    let mut catalog = built(&WeakDom::new());
    let wood = slot(&mut catalog, &material_of("Wood"));
    catalog.resolve(HashMap::new());

    let maps = catalog.resolved_maps();
    assert_eq!(maps.len(), 2);
    assert_eq!(maps[wood.layer as usize], [None, None, None, None]);
}

// A material the scene had not used before is a new layer, which means new
// texels in every array: the key has to move, while the layers already there
// stay exactly where they were.
#[test]
fn resolved_maps_change_when_a_material_joins_the_scene() {
    let mut catalog = built(&WeakDom::new());
    slot(&mut catalog, &material_of("Wood"));
    let before = catalog.resolved_maps();

    slot(&mut catalog, &material_of("Brick"));

    let after = catalog.resolved_maps();
    assert_ne!(after, before);
    assert_eq!(after[..before.len()], before[..]);
}
