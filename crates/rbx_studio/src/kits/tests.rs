use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use glam::Vec3;
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};
use serde_json::json;

use super::import::{bounds, shape};
use super::local::{is_cc0, origin, scale_for, KENNEY_CITY_SCALE, KENNEY_SCALE};
use super::polyhaven::{enclosed, group};
use super::{add_pack, resolve, title, Credit, Provider, Thumb, METRES, TEST_ROOT};

/// A fresh kit cache for this test alone.
fn cache(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx_kits_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TEST_ROOT.with(|root| *root.borrow_mut() = Some(dir.clone()));
    dir
}

fn zip(path: &Path, files: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn at(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

fn part(dom: &mut WeakDom, name: &str, centre: (f32, f32, f32), size: f32) -> rbx_dom::Ref {
    let part = dom.new_instance("MeshPart", name, None);
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(at(centre.0, centre.1, centre.2)),
    )
    .unwrap();
    dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: size,
            y: size,
            z: size,
        }),
    )
    .unwrap();
    part
}

#[test]
fn names_read_as_titles() {
    assert_eq!(title("platformer-kit"), "Platformer Kit");
    assert_eq!(title("tree_pine-tall"), "Tree Pine Tall");
    assert_eq!(title("project_lighthouse"), "Project Lighthouse");
}

#[test]
fn a_shaped_import_is_one_scaled_model_standing_on_the_origin_with_credit() {
    let mut dom = WeakDom::new();
    // Two loose parts, as an import without a Model of its own holds: one
    // unit cubes, side by side, floating 3 units up.
    part(&mut dom, "Left", (10.0, 3.5, 0.0), 1.0);
    part(&mut dom, "Right", (11.0, 3.5, 0.0), 1.0);
    let credit = Credit {
        source: "https://polyhaven.com/a/crate".into(),
        author: "Someone".into(),
        license: Some("CC0"),
    };
    let (model, size) = shape(&mut dom, "Crate", &credit, 4.0).unwrap();

    assert_eq!(dom.root_refs(), [model]);
    let instance = dom.get(model).unwrap();
    assert_eq!((instance.class(), instance.name()), ("Model", "Crate"));
    assert_eq!(instance.children().len(), 2);
    assert_eq!(size, Vec3::new(8.0, 4.0, 4.0));
    let (min, max) = bounds(&dom, &[model]).unwrap();
    assert_eq!(min, Vec3::new(-4.0, 0.0, -2.0));
    assert_eq!(max, Vec3::new(4.0, 4.0, 2.0));
    assert_eq!(
        instance.properties().get("WorldPivotData"),
        Some(&Variant::OptionalCFrame(Some(at(0.0, 0.0, 0.0))))
    );
    for &child in instance.children() {
        assert_eq!(
            dom.get(child).unwrap().properties().get("Anchored"),
            Some(&Variant::Bool(true))
        );
    }
    let attributes = rbx_dom::attributes::decode(instance.properties().get("AttributesSerialize"));
    assert_eq!(
        attributes.get("Source"),
        Some(&Variant::String(credit.source.clone()))
    );
    assert_eq!(
        attributes.get("License"),
        Some(&Variant::String("CC0".into()))
    );
    assert_eq!(
        attributes.get("Author"),
        Some(&Variant::String("Someone".into()))
    );
}

#[test]
fn an_import_already_in_a_model_keeps_it_and_takes_the_asset_name() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Scene", None);
    let piece = part(&mut dom, "Mesh", (0.0, 0.5, 0.0), 1.0);
    dom.set_parent(piece, Some(model));
    let credit = Credit {
        source: String::new(),
        author: String::new(),
        license: None,
    };
    let (root, _) = shape(&mut dom, "Barrel", &credit, 1.0).unwrap();
    assert_eq!(root, model);
    assert_eq!(dom.get(root).unwrap().name(), "Barrel");
    assert!(shape(&mut WeakDom::new(), "Empty", &credit, 1.0).is_err());
}

#[test]
fn only_a_cc0_licence_passes_and_kenney_zips_name_their_page() {
    assert!(is_cc0("License: (Creative Commons Zero, CC0)"));
    assert!(is_cc0("https://creativecommons.org/publicdomain/zero/1.0/"));
    assert!(!is_cc0("Quaternius Asset License (QAL) v1.0"));
    assert!(!is_cc0("CC-BY 4.0"));
    assert_eq!(
        origin("kenney_platformer-kit", "CC0"),
        (
            "Kenney".to_owned(),
            "https://kenney.nl/assets/platformer-kit".to_owned()
        )
    );
    assert_eq!(origin("pack", "CC0, by Quaternius").0, "Quaternius");
    assert_eq!(scale_for("Kenney", "City Kit Roads"), KENNEY_CITY_SCALE);
    assert_eq!(scale_for("Kenney", "Platformer Kit"), KENNEY_SCALE);
    assert_eq!(scale_for("Unknown", "Things"), METRES);
}

#[test]
fn an_added_zip_unpacks_its_models_with_previews_and_resolves_them() {
    let dir = cache("zip");
    let path = dir.join("kenney_platformer-kit.zip");
    zip(
        &path,
        &[
            ("License.txt", b"Creative Commons Zero, CC0"),
            ("Models/GLB format/coin-gold.glb", b"glb"),
            ("Models/GLB format/Textures/colormap.png", b"png"),
            ("Models/OBJ format/coin-gold.obj", b"obj"),
            ("Models/FBX format/flag.fbx", b"fbx"),
            ("Previews/coin-gold.png", b"png"),
        ],
    );
    let pack = add_pack(&path).unwrap();
    assert_eq!(pack.name, "Platformer Kit");
    assert_eq!(pack.author, "Kenney");
    let ids: Vec<&str> = pack.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "kenney_platformer-kit/coin-gold",
            "kenney_platformer-kit/flag"
        ]
    );
    assert!(
        matches!(&pack.models[0].thumb, Thumb::File(p) if p.ends_with("Previews/coin-gold.png"))
    );
    assert_eq!(pack.models[1].thumb, Thumb::None);

    let resolved = resolve(Provider::Local, "kenney_platformer-kit/coin-gold").unwrap();
    assert!(resolved.file.ends_with("Models/GLB format/coin-gold.glb"));
    assert_eq!(resolved.name, "Coin Gold");
    assert_eq!(resolved.scale, KENNEY_SCALE);
    assert_eq!(
        resolved.credit.source,
        "https://kenney.nl/assets/platformer-kit"
    );
    assert_eq!(super::local::packs().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_zip_without_a_cc0_licence_is_refused_and_nothing_is_unpacked() {
    let dir = cache("refused");
    let bare = dir.join("bare.zip");
    zip(&bare, &[("tree.glb", b"glb")]);
    assert!(add_pack(&bare).unwrap_err().contains("no licence file"));
    let qal = dir.join("quaternius.zip");
    zip(
        &qal,
        &[
            ("License.txt", b"Quaternius Asset License"),
            ("tree.glb", b"glb"),
        ],
    );
    assert!(add_pack(&qal).unwrap_err().contains("isn\u{2019}t CC0"));
    assert!(super::local::packs().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn poly_haven_groups_by_collection_then_by_first_category() {
    let catalogue: BTreeMap<String, serde_json::Value> = [
        (
            "barrel_01",
            json!({ "name": "Barrel 01", "categories": ["props", "containers", "collection: the_shed"] }),
        ),
        ("crate_02", json!({ "name": "Crate 02", "categories": ["props"] })),
        ("rock_03", json!({ "name": "Rock 03", "categories": ["nature"] })),
    ]
    .into_iter()
    .map(|(slug, asset)| (slug.to_owned(), asset))
    .collect();
    let packs = group(&catalogue);
    let names: Vec<(&str, usize)> = packs
        .iter()
        .map(|p| (p.name.as_str(), p.models.len()))
        .collect();
    assert_eq!(names, [("The Shed", 1), ("Props", 2), ("Nature", 1)]);
    assert_eq!(packs[0].id, "polyhaven:collection/the_shed");
    assert_eq!(
        packs[0].models[0].thumb,
        Thumb::Url(
            "https://cdn.polyhaven.com/asset_img/thumbs/barrel_01.png?width=256&height=256".into()
        )
    );
}

#[test]
fn a_side_file_path_must_stay_inside_the_model_folder() {
    assert_eq!(
        enclosed("textures/a.jpg"),
        Some(PathBuf::from("textures/a.jpg"))
    );
    assert_eq!(enclosed("../escape.bin"), None);
    assert_eq!(enclosed("/etc/passwd"), None);
}
