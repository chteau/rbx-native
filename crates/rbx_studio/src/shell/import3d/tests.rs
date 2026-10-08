use std::path::Path;

use rbx_dom::{Ref, WeakDom};

use super::{is_image, place_model};

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
