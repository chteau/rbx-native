use super::*;
use rbx_dom::{CFrameData, Instance, Variant, Vector3Data};

/// A `Workspace` with one `Part` carrying a `Decal` whose `Texture` names a
/// package `rbxasset://` never groups its content under — resolving it
/// fails locally (`AssetError::UnknownNativePackage`, see
/// `rbx_assets::native::package_candidates_for_path`) before any network
/// call would be attempted, which is what keeps this test offline-safe.
fn dom_with_unresolvable_decal() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9100);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);

    let part_ref = Ref::new(9101);
    let mut part = Instance::new(part_ref, "Part", "Part");
    part.properties_mut().insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 4.0,
            z: 4.0,
        }),
    );
    part.properties_mut().insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    dom.insert(part);
    dom.set_parent(part_ref, Some(workspace));

    let decal_ref = Ref::new(9102);
    let mut decal = Instance::new(decal_ref, "Decal", "Decal");
    let properties = decal.properties_mut();
    properties.insert(
        "Texture".to_string(),
        Variant::String("rbxasset://unknown-native-package/none.png".to_string()),
    );
    properties.insert("Face".to_string(), Variant::Enum(0));
    dom.insert(decal);
    dom.set_parent(decal_ref, Some(part_ref));

    dom
}

/// The same place, with a `ParticleEmitter` in place of the `Decal`. An
/// emitter's texture is fetched down a different branch of `Loaded::resolve`
/// from every other image (`resolve_effect_images`), because the render pass
/// reads a pre-computed answer rather than resolving one itself.
fn dom_with_unresolvable_particle_emitter() -> WeakDom {
    let mut dom = dom_with_unresolvable_decal();
    let decal = Ref::new(9102);
    dom.remove(decal);

    let emitter_ref = Ref::new(9103);
    let mut emitter = Instance::new(emitter_ref, "ParticleEmitter", "ParticleEmitter");
    emitter.properties_mut().insert(
        "Texture".to_string(),
        Variant::String("rbxasset://unknown-native-package/none.png".to_string()),
    );
    dom.insert(emitter);
    dom.set_parent(emitter_ref, Some(Ref::new(9101)));

    dom
}

fn textures_only() -> Toggles {
    Toggles {
        textures: true,
        materials: false,
        lights: false,
        clock_time: None,
        show_development_gui: false,
    }
}

#[test]
fn from_dom_surfaces_a_warning_for_a_decal_that_cannot_resolve() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_decal();

    let mut loaded = Loaded::from_dom(&dom, &database, textures_only(), &mut Resident::default())
        .expect("scene should still load");
    let warnings = loaded.take_warnings();

    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("unknown-native-package")),
        "expected a warning naming the failed asset, got {warnings:?}"
    );
    // A second drain finds nothing: a `Loaded` yields its warnings once.
    assert!(loaded.take_warnings().is_empty());
}

/// An emitter's texture reaches the pass as `Some(None)` — "tried, and there
/// is no image" — with the reason already thrown away, so if the warning is
/// not carried out of `resolve_effect_images` it is lost for good and the
/// dock shows nothing at all for a texture that will never resolve.
#[test]
fn from_dom_surfaces_a_warning_for_an_emitter_texture_that_cannot_resolve() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_particle_emitter();

    let mut loaded = Loaded::from_dom(&dom, &database, textures_only(), &mut Resident::default())
        .expect("scene should still load");
    let warnings = loaded.take_warnings();

    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("unknown-native-package")),
        "expected a warning naming the failed asset, got {warnings:?}"
    );
}

// The failure that is nobody's in particular — no resolver could be built
// at all — still has to land in the dock, not only on stderr.
#[test]
fn from_dom_surfaces_a_warning_when_no_resolver_can_be_built() {
    let _failure = crate::assets::tests::ResolverFailure::new("cache dir is a file");
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_decal();

    let mut loaded = Loaded::from_dom(&dom, &database, textures_only(), &mut Resident::default())
        .expect("scene should still load");
    let warnings = loaded.take_warnings();

    assert_eq!(
        warnings,
        vec!["rbxview: no textures (cache dir is a file)".to_string()]
    );
}

// A transient failure is retried by the next load, not remembered for the
// life of the `Resident`: once the machine is fixed, the reload fetches what
// the load could not — here the warning changes from "no resolver" to the
// asset's own, which only a second fetch can produce.
#[test]
fn a_reload_retries_what_the_previous_load_failed_to_fetch() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_decal();
    let mut resident = Resident::default();

    let no_resolver = crate::assets::tests::ResolverFailure::new("cache dir is a file");
    let mut first =
        Loaded::from_dom(&dom, &database, textures_only(), &mut resident).expect("load");
    assert_eq!(
        first.take_warnings(),
        vec!["rbxview: no textures (cache dir is a file)".to_string()]
    );
    drop(no_resolver);

    let mut again =
        Loaded::from_dom(&dom, &database, textures_only(), &mut resident).expect("reload");
    let warnings = again.take_warnings();

    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("unknown-native-package")),
        "expected the asset to be fetched again, got {warnings:?}"
    );
}

// The streaming counterpart: the same place comes back drawable with the
// fetch still running, and the warning arrives from the loader's own poll
// once the resolve has failed for good rather than from the build.
#[test]
fn a_streaming_build_returns_before_the_warning_exists() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_decal();
    let mut resident = Resident::streaming();

    let mut loaded = Loaded::from_dom(&dom, &database, textures_only(), &mut resident)
        .expect("scene should still load");

    assert!(
        loaded.take_warnings().is_empty(),
        "a streaming build never waits long enough to have a warning"
    );
    assert_eq!(loaded.scene().parts().len(), 1, "the part is drawable now");

    let settled = resident.settle(std::time::Duration::from_secs(10));
    assert!(
        settled
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown-native-package")),
        "the failure is reported once it is one, got {:?}",
        settled.warnings
    );
    // And the reference the place is waiting on is recognised as its own.
    assert!(loaded.wants_any(&settled.references));
}

#[test]
fn a_landing_for_an_asset_the_place_never_named_is_ignored() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_unresolvable_decal();
    let mut resident = Resident::streaming();
    let loaded =
        Loaded::from_dom(&dom, &database, textures_only(), &mut resident).expect("scene loads");

    assert!(!loaded.wants_any(&[rbx_assets::AssetRef::Id(987_654_321)]));
}

#[test]
fn read_place_sniffs_xml_and_builds_a_part() {
    let xml = r#"<roblox version="4"><Item class="Workspace" referent="RBX0"><Properties><string name="Name">Workspace</string></Properties><Item class="Part" referent="RBX1"><Properties><string name="Name">P</string><Vector3 name="size"><X>4</X><Y>1</Y><Z>2</Z></Vector3><CoordinateFrame name="CFrame"><X>0</X><Y>0</Y><Z>0</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame></Properties></Item></Item></roblox>"#;

    let dir = std::env::temp_dir();
    let path = dir.join("rbx_viewer_read_place_test.rbxlx");
    std::fs::write(&path, xml).expect("write temp fixture");

    let dom = read_place(&path).expect("xml place should parse");
    std::fs::remove_file(&path).ok();

    let workspace = dom.get(dom.root_refs()[0]).expect("root instance");
    assert_eq!(workspace.class(), "Workspace");
    let part = dom.get(workspace.children()[0]).expect("part instance");
    assert_eq!(part.class(), "Part");
    assert_eq!(part.name(), "P");
}

// Stands in for `Headless::reload`, which needs a GPU: this is the shared
// pipeline it calls, so proving it reacts to a mutated DOM is proving the
// reload path works without one.
#[test]
fn from_dom_reflects_a_property_mutated_after_the_file_was_read() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let mut dom = read_place(&fixture).expect("fixture should parse");
    let toggles = Toggles {
        textures: false,
        materials: false,
        lights: false,
        clock_time: None,
        show_development_gui: false,
    };

    let part = crate::scene::descendants(&dom)
        .find(|referent| {
            dom.get(*referent)
                .is_some_and(|instance| instance.properties().contains_key("size"))
        })
        .expect("fixture should have a sized part");

    let database = ReflectionDatabase::embedded();
    let mut resident = Resident::default();
    let before = Loaded::from_dom(&dom, &database, toggles, &mut resident).expect("first load");
    let before_corners = before.world().scene.bounds().corners();

    // Grown far past whatever the fixture already spans, so the bounds
    // change is unambiguous however the part sat in the scene.
    dom.set_property(
        part,
        "size",
        rbx_dom::Variant::Vector3(rbx_dom::Vector3Data {
            x: 500.0,
            y: 500.0,
            z: 500.0,
        }),
    )
    .expect("the fixture part should still exist");

    let after =
        Loaded::from_dom(&dom, &database, toggles, &mut resident).expect("reload after mutation");
    let after_corners = after.world().scene.bounds().corners();

    assert_eq!(
        before.world().scene.parts().len(),
        after.world().scene.parts().len(),
        "mutating a property must not add or remove parts"
    );
    assert_ne!(before_corners, after_corners);
}

// Re-resolving must be a no-op when nothing new has landed: the place is
// joined to the same assets again and must come out unchanged, not with a
// union's recovered pieces or a decal group drawn twice.
#[test]
fn resolving_again_with_nothing_new_changes_nothing() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let dom = read_place(&fixture).expect("fixture should parse");
    let database = ReflectionDatabase::embedded();
    let toggles = Toggles {
        textures: false,
        materials: false,
        lights: false,
        clock_time: None,
        show_development_gui: false,
    };
    let mut resident = Resident::default();
    let mut loaded = Loaded::from_dom(&dom, &database, toggles, &mut resident).expect("load");

    let parts = loaded.scene().parts().len();
    let instances = loaded.scene().resolved_file_meshes().instances.len();
    let groups = loaded.world().decor.groups.len();

    for _ in 0..3 {
        loaded.resolve(&mut resident);
    }

    assert_eq!(loaded.scene().parts().len(), parts);
    assert_eq!(
        loaded.scene().resolved_file_meshes().instances.len(),
        instances
    );
    assert_eq!(loaded.world().decor.groups.len(), groups);
}

/// A source with one font family in it and nothing else: the family JSON
/// under its `rbxasset://` path and the single face it names.
struct FontShelf;

impl Source for FontShelf {
    fn image(&self, reference: &AssetRef) -> Result<crate::assets::Image, crate::assets::Failure> {
        Err(not_here(reference))
    }

    fn mesh(&self, reference: &AssetRef) -> Result<rbx_mesh::Mesh, crate::assets::Failure> {
        Err(not_here(reference))
    }

    fn bytes(&self, reference: &AssetRef) -> Result<Vec<u8>, crate::assets::Failure> {
        match reference {
            AssetRef::Native(path) if path == "fonts/families/Shelf.json" => Ok(br#"{
                "name": "Shelf",
                "faces": [
                    {"name": "Regular", "weight": 400, "style": "normal", "assetId": "rbxasset://fonts/Shelf-Regular.ttf"},
                    {"name": "Bold", "weight": 700, "style": "normal", "assetId": "rbxasset://fonts/Shelf-Bold.ttf"}
                ]
            }"#
            .to_vec()),
            AssetRef::Native(path) if path == "fonts/Shelf-Bold.ttf" => Ok(vec![7, 0, 0]),
            _ => Err(not_here(reference)),
        }
    }
}

fn not_here(reference: &AssetRef) -> crate::assets::Failure {
    crate::assets::Failure {
        warning: format!("{reference:?}: not on the shelf"),
        transient: false,
    }
}

/// [`dom_with_unresolvable_decal`] plus a `ScreenGui` holding one bold
/// `TextLabel` in the shelf's family.
fn dom_with_a_text_label() -> WeakDom {
    let mut dom = dom_with_unresolvable_decal();
    let starter = Ref::new(9200);
    dom.insert(Instance::new(starter, "StarterGui", "StarterGui"));
    dom.set_parent(starter, None);
    let screen = Ref::new(9201);
    dom.insert(Instance::new(screen, "ScreenGui", "ScreenGui"));
    dom.set_parent(screen, Some(starter));
    let label = Ref::new(9202);
    let mut label_instance = Instance::new(label, "TextLabel", "TextLabel");
    let properties = label_instance.properties_mut();
    properties.insert("Text".to_string(), Variant::String("bold".to_string()));
    properties.insert(
        "FontFace".to_string(),
        Variant::Font(rbx_dom::Font {
            family: "rbxasset://fonts/families/Shelf.json".to_string(),
            weight: 700,
            style: rbx_dom::FontStyle::Normal,
            cached_face_id: None,
        }),
    );
    dom.insert(label_instance);
    dom.set_parent(label, Some(screen));
    dom
}

// A font is two fetches deep — the family's JSON, then the face it names —
// and a streaming loader has to carry the request across two landings: the
// first resolve asks for the family, the resolve after it lands asks for the
// face, and only the resolve after *that* has bytes for the renderer.
#[test]
fn a_streaming_load_lands_a_font_family_and_then_its_face() {
    let database = ReflectionDatabase::embedded();
    let dom = dom_with_a_text_label();
    let mut resident = Resident::fed_by(std::sync::Arc::new(FontShelf));
    let nothing = Toggles {
        textures: false,
        materials: false,
        lights: false,
        clock_time: None,
        show_development_gui: false,
    };
    let bold = crate::fonts::Face::named("Shelf", 700, false);
    let patience = std::time::Duration::from_secs(5);

    let mut loaded = Loaded::from_dom(&dom, &database, nothing, &mut resident).expect("load");
    assert!(loaded.world().fonts.families.is_empty());
    assert!(loaded.world().fonts.bytes_of(&bold).is_none());

    let landed = resident.settle(patience);
    assert!(
        loaded.wants_any(&landed.references),
        "the family was asked for"
    );
    loaded.resolve(&mut resident);
    assert_eq!(loaded.world().fonts.families.len(), 1);
    assert!(loaded.world().fonts.bytes_of(&bold).is_none());

    let landed = resident.settle(patience);
    assert!(
        loaded.wants_any(&landed.references),
        "the face was asked for"
    );
    loaded.resolve(&mut resident);
    let (asset, bytes) = loaded
        .world()
        .fonts
        .bytes_of(&bold)
        .expect("the face is in");
    assert_eq!(asset, &AssetRef::Native("fonts/Shelf-Bold.ttf".to_string()));
    assert_eq!(bytes.as_slice(), &[7, 0, 0]);
    assert_eq!(resident.in_flight(), 0, "the regular face was never wanted");
}
