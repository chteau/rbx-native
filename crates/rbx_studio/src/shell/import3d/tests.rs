use std::path::Path;

use rbx_dom::{Ref, WeakDom};

use super::{is_image, place_meshes, place_model};

fn imported(roots: &[(&str, &str)]) -> WeakDom {
    let mut dom = WeakDom::new();
    for &(class, name) in roots {
        let root = dom.new_instance(class, name, None);
        dom.new_instance("MeshPart", "Body", Some(root));
    }
    dom
}

fn names_under(dom: &WeakDom, parent: Ref) -> Vec<String> {
    dom.get(parent)
        .unwrap()
        .children()
        .iter()
        .map(|&c| dom.get(c).unwrap().name().to_string())
        .collect()
}

#[test]
fn a_single_model_lands_renamed_with_its_meshparts() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let root = place_model(
        &mut dom,
        &imported(&[("Model", "Model")]),
        "chair",
        workspace,
    )
    .unwrap();
    assert_eq!(dom.get(root).unwrap().name(), "chair");
    assert_eq!(dom.get(root).unwrap().class(), "Model");
    assert_eq!(names_under(&dom, root), ["Body"]);
    assert_eq!(dom.parent(root), Some(workspace));
}

#[test]
fn several_top_level_instances_are_held_in_a_model() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let source = imported(&[("MeshPart", "A"), ("MeshPart", "B")]);
    let root = place_model(&mut dom, &source, "pair", workspace).unwrap();
    assert_eq!(dom.get(root).unwrap().class(), "Model");
    assert_eq!(names_under(&dom, root), ["A", "B"]);
}

#[test]
fn an_empty_import_places_nothing() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    assert!(place_model(&mut dom, &WeakDom::new(), "x", workspace).is_none());
}

#[test]
fn files_are_told_apart_by_extension() {
    assert!(is_image(Path::new("a/Tex.PNG")));
    assert!(!is_image(Path::new("a.fbx")));
}

#[test]
fn meshes_land_alone_or_in_a_model() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let one = place_meshes(
        &mut dom,
        &imported(&[("Model", "Model")]),
        "chair",
        workspace,
    )
    .unwrap();
    assert_eq!(dom.get(one).unwrap().class(), "MeshPart");
    assert_eq!(dom.get(one).unwrap().name(), "chair");
    let two = place_meshes(
        &mut dom,
        &imported(&[("Model", "A"), ("Folder", "B")]),
        "set",
        workspace,
    )
    .unwrap();
    assert_eq!(dom.get(two).unwrap().class(), "Model");
    assert_eq!(dom.get(two).unwrap().children().len(), 2);
    assert!(place_meshes(&mut dom, &WeakDom::new(), "none", workspace).is_none());
}

/// Live: uploads a one-triangle glTF through Open Cloud and checks a
/// `MeshPart` comes back. Needs a key (`RBX_API_KEY` or the keyring):
/// `cargo test -p rbx_studio live_upload -- --ignored --nocapture`
#[test]
#[ignore]
fn live_upload_round_trip() {
    const TRIANGLE: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"bufferViews":[{"buffer":0,"byteLength":36}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}"#;
    let path = std::env::temp_dir().join("rbx_native_live_triangle.gltf");
    std::fs::write(&path, TRIANGLE).unwrap();
    let imported = super::upload_model(&path, "rbx-native live test").unwrap();
    let mut place = WeakDom::new();
    let workspace = place.new_instance("Workspace", "Workspace", None);
    let has_mesh = place_meshes(&mut place, &imported.dom, "t", workspace).is_some();
    eprintln!("asset {}: MeshPart present = {has_mesh}", imported.asset_id);
    assert!(has_mesh);
}
