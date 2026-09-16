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

fn textures_only() -> Toggles {
    Toggles {
        textures: true,
        materials: false,
        lights: false,
        clock_time: None,
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
