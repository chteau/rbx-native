//! `SurfaceAppearance` planning and resolution, on the same fixtures the
//! geometry tests use.

use super::*;

/// A `MeshPart` with one `SurfaceAppearance` child carrying `properties`.
fn with_appearance(properties: Vec<(&str, Variant)>) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let part_ref = Ref::new(1);
    let mut part = Instance::new(part_ref, "MeshPart", "Rock");
    let owned = part.properties_mut();
    owned.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    owned.insert(
        "TextureID".to_string(),
        Variant::String("rbxassetid://2".to_string()),
    );
    owned.insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    owned.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(part);
    dom.set_parent(part_ref, Some(workspace));

    let child_ref = Ref::new(2);
    let mut child = Instance::new(child_ref, "SurfaceAppearance", "SurfaceAppearance");
    for (name, value) in properties {
        child.properties_mut().insert(name.to_string(), value);
    }
    dom.insert(child);
    dom.set_parent(child_ref, Some(part_ref));

    dom
}

fn content(id: u64) -> Variant {
    Variant::Content(rbx_dom::Content::Uri(format!("rbxassetid://{id}")))
}

#[test]
fn all_four_maps_are_planned_in_map_kind_order() {
    let dom = with_appearance(vec![
        ("ColorMap", content(10)),
        ("NormalMap", content(11)),
        ("MetalnessMap", content(12)),
        ("RoughnessMap", content(13)),
    ]);

    let plan = planned(&dom);
    let appearance = plan.entries[0]
        .appearance
        .as_ref()
        .expect("the child is a SurfaceAppearance");

    assert_eq!(
        appearance.maps,
        [10, 11, 12, 13].map(|id| Some(AssetRef::Id(id)))
    );
    // The mesh's own TextureID is replaced, not layered under.
    assert_eq!(plan.entries[0].texture, None);
    assert_eq!(
        plan.texture_refs(),
        vec![
            AssetRef::Id(10),
            AssetRef::Id(11),
            AssetRef::Id(12),
            AssetRef::Id(13)
        ]
    );
}

#[test]
fn a_set_with_only_a_colour_map_leaves_the_other_three_empty() {
    let dom = with_appearance(vec![
        ("ColorMap", content(10)),
        // Roblox writes an empty string for a map the author left blank.
        ("NormalMap", Variant::String(String::new())),
    ]);

    let appearance = planned(&dom).entries[0].appearance.clone().unwrap();

    assert_eq!(appearance.maps[0], Some(AssetRef::Id(10)));
    assert!(appearance.maps[1..].iter().all(Option::is_none));
}

#[test]
fn the_alpha_mode_enum_is_read_and_defaults_to_overlay() {
    let transparency = with_appearance(vec![
        ("ColorMap", content(10)),
        ("AlphaMode", Variant::Enum(1)),
    ]);
    let overlay = with_appearance(vec![
        ("ColorMap", content(10)),
        ("AlphaMode", Variant::Enum(0)),
    ]);
    let absent = with_appearance(vec![("ColorMap", content(10))]);

    let mode = |dom: &WeakDom| {
        planned(dom).entries[0]
            .appearance
            .clone()
            .unwrap()
            .alpha_mode
    };
    assert_eq!(mode(&transparency), AlphaMode::Transparency);
    assert_eq!(mode(&overlay), AlphaMode::Overlay);
    assert_eq!(mode(&absent), AlphaMode::Overlay);
}

#[test]
fn the_colour_tint_is_linearized_and_white_by_default() {
    let tinted = with_appearance(vec![(
        "Color",
        Variant::Color3(rbx_dom::Color3Data {
            r: 1.0,
            g: 0.5,
            b: 0.0,
        }),
    )]);

    let tint = planned(&tinted).entries[0].appearance.clone().unwrap().tint;
    assert_eq!(tint[0], 1.0);
    assert!((tint[1] - crate::scene::srgb_to_linear(0.5)).abs() < 1e-6);
    assert_eq!(tint[2], 0.0);

    let plain = with_appearance(Vec::new());
    assert_eq!(
        planned(&plain).entries[0].appearance.clone().unwrap().tint,
        [1.0; 3]
    );
}

#[test]
fn resolve_drops_the_maps_that_never_downloaded_and_deduplicates_the_sets() {
    let mut dom = with_appearance(vec![
        ("ColorMap", content(10)),
        ("NormalMap", content(11)),
        ("AlphaMode", Variant::Enum(1)),
    ]);
    // A second part wearing an identical appearance, which must share a slot.
    let twin_ref = Ref::new(3);
    let mut twin = Instance::new(twin_ref, "MeshPart", "Rock");
    let owned = twin.properties_mut();
    owned.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    owned.insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    owned.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(twin);
    dom.set_parent(twin_ref, Some(Ref::new(9000)));
    let appearance_ref = Ref::new(4);
    let mut appearance = Instance::new(appearance_ref, "SurfaceAppearance", "SurfaceAppearance");
    let owned = appearance.properties_mut();
    owned.insert("ColorMap".to_string(), content(10));
    owned.insert("NormalMap".to_string(), content(11));
    owned.insert("AlphaMode".to_string(), Variant::Enum(1));
    dom.insert(appearance);
    dom.set_parent(appearance_ref, Some(twin_ref));

    let plan = planned(&dom);
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh([1.0, 1.0, 1.0])));
    let mut images = HashMap::new();
    // Only the normal map arrived, so the alpha mode has nothing left to blend.
    images.insert(
        AssetRef::Id(11),
        Arc::new(crate::assets::Image {
            width: 1,
            height: 1,
            pixels: vec![128, 128, 255, 255],
        }),
    );

    let (resolved, _) = resolve(&plan, meshes, images);

    assert_eq!(resolved.appearances.len(), 1);
    assert!(resolved
        .instances
        .iter()
        .all(|instance| instance.appearance == Some(0)));
    assert_eq!(resolved.appearances[0].maps[0], None);
    assert_eq!(resolved.appearances[0].maps[1], Some(AssetRef::Id(11)));
    assert_eq!(resolved.appearances[0].alpha_mode, AlphaMode::Overlay);
    assert!(!resolved.appearances[0].is_translucent(&resolved.images));
}

#[test]
fn only_a_transparency_set_whose_colour_map_has_alpha_blends() {
    let translucent = |alpha: u8, mode: u32| {
        let dom = with_appearance(vec![
            ("ColorMap", content(10)),
            ("AlphaMode", Variant::Enum(mode)),
        ]);
        let plan = planned(&dom);
        let mut meshes = HashMap::new();
        meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh([1.0, 1.0, 1.0])));
        let mut images = HashMap::new();
        images.insert(
            AssetRef::Id(10),
            Arc::new(crate::assets::Image {
                width: 1,
                height: 1,
                pixels: vec![255, 255, 255, alpha],
            }),
        );
        let (resolved, _) = resolve(&plan, meshes, images);
        resolved.appearances[0].is_translucent(&resolved.images)
    };

    assert!(translucent(128, 1));
    // An opaque map blends nothing however its mode reads.
    assert!(!translucent(255, 1));
    assert!(!translucent(128, 0));
}

#[test]
fn a_part_carrying_two_appearances_keeps_the_first_as_studio_does() {
    let mut dom = with_appearance(vec![("ColorMap", content(10))]);
    let second_ref = Ref::new(3);
    let mut second = Instance::new(second_ref, "SurfaceAppearance", "SurfaceAppearance");
    second
        .properties_mut()
        .insert("ColorMap".to_string(), content(99));
    dom.insert(second);
    dom.set_parent(second_ref, Some(Ref::new(1)));

    let appearance = planned(&dom).entries[0].appearance.clone().unwrap();
    assert_eq!(appearance.maps[0], Some(AssetRef::Id(10)));
}
