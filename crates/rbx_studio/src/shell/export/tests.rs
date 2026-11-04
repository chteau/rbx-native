use rbx_dom::Variant;

use super::*;
use crate::explorer::find_by_name;

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// A model with a part, and two `ObjectValue`s: one pointing inside the
/// model, one at a part left outside it.
fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let outside = dom.new_instance("Part", "Outside", Some(workspace));
    let model = dom.new_instance("Model", "Car", Some(workspace));
    let wheel = dom.new_instance("Part", "Wheel", Some(model));
    let inner = dom.new_instance("ObjectValue", "Inner", Some(model));
    let outer = dom.new_instance("ObjectValue", "Outer", Some(model));
    dom.set_property(inner, "Value", Variant::Ref(wheel))
        .unwrap();
    dom.set_property(outer, "Value", Variant::Ref(outside))
        .unwrap();
    (dom, model, wheel)
}

fn value_of(dom: &WeakDom, name: &str) -> Option<Variant> {
    let referent = find_by_name(dom, name)?;
    dom.get(referent)?.properties().get("Value").cloned()
}

#[test]
fn save_to_file_writes_the_subtree_as_a_binary_model() {
    let (dom, model, wheel) = place();

    let bytes = encode(
        Export::Model,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[model, wheel],
        Path::new("Car.rbxm"),
    )
    .unwrap()
    .remove(0)
    .1;
    let read = rbx_binary::deserialize(&bytes).unwrap();

    assert_eq!(
        read.root_refs().len(),
        1,
        "the wheel comes with its model, once"
    );
    let root = read.get(read.root_refs()[0]).unwrap();
    assert_eq!((root.class(), root.name()), ("Model", "Car"));
    assert!(find_by_name(&read, "Outside").is_none());
    assert_eq!(
        value_of(&read, "Inner"),
        Some(Variant::Ref(find_by_name(&read, "Wheel").unwrap()))
    );
    assert_eq!(
        value_of(&read, "Outer"),
        None,
        "a reference left behind is nil"
    );
}

#[test]
fn an_rbxmx_name_saves_xml() {
    let (dom, model, _) = place();

    let bytes = encode(
        Export::Model,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[model],
        Path::new("Car.rbxmx"),
    )
    .unwrap()
    .remove(0)
    .1;

    let read = rbx_xml::deserialize(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert!(find_by_name(&read, "Wheel").is_some());
}

#[test]
fn a_mesh_export_of_no_parts_is_an_error_not_an_empty_file() {
    let (mut dom, _, _) = place();
    let folder = dom.new_instance("Folder", "Empty", None);

    for kind in [Export::Obj, Export::Gltf] {
        let result = encode(
            kind,
            &dom,
            &database(),
            &pick::Meshes::default(),
            &[folder],
            Path::new("x"),
        );
        assert!(result.is_err());
    }
    assert!(!has_geometry(&dom, &database(), &[folder]));
}

/// File › Save to File… writes every service, not a selection: binary by
/// default, XML under an `.rbxlx` name.
#[test]
fn save_to_file_writes_the_whole_place_in_the_format_its_name_asks_for() {
    let (mut dom, _, _) = place();
    dom.new_instance("Lighting", "Lighting", None);

    for (name, xml) in [("Copy.rbxl", false), ("Copy.rbxlx", true)] {
        let files = encode(
            Export::Place,
            &dom,
            &database(),
            &pick::Meshes::default(),
            &[],
            Path::new(name),
        )
        .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, Path::new(name));
        let read = if xml {
            rbx_xml::deserialize(std::str::from_utf8(&files[0].1).unwrap()).unwrap()
        } else {
            rbx_binary::deserialize(&files[0].1).unwrap()
        };
        let mut roots: Vec<_> = read
            .root_refs()
            .iter()
            .map(|&r| read.get(r).unwrap().class().to_owned())
            .collect();
        roots.sort();
        assert_eq!(roots, ["Lighting", "Workspace"]);
        assert!(find_by_name(&read, "Wheel").is_some());
    }
}

/// File › Export Clean Copy… writes the place with no trace of the
/// editor's layers or notes, in either format, and leaves the open place
/// holding them.
#[test]
fn a_clean_copy_carries_no_editor_data_and_the_open_place_keeps_it() {
    let (mut dom, model, _) = place();
    let layer = crate::layers::add_layer(&mut dom, "Props", (1, 2, 3)).unwrap();
    crate::layers::assign(&mut dom, &[model], Some(&layer)).unwrap();
    crate::notes::add_note(&mut dom, "Fix", "me", (4, 5, 6), [0.0; 3], Some(model)).unwrap();

    for name in ["Clean.rbxl", "Clean.rbxlx"] {
        let files = encode(
            Export::CleanPlace,
            &dom,
            &database(),
            &pick::Meshes::default(),
            &[],
            Path::new(name),
        )
        .unwrap();
        let bytes = &files[0].1;
        let marker = crate::layers::PREFIX.as_bytes();
        assert!(!bytes.windows(marker.len()).any(|window| window == marker));
        let read = match name.ends_with('x') {
            true => rbx_xml::deserialize(std::str::from_utf8(bytes).unwrap()).unwrap(),
            false => rbx_binary::deserialize(bytes).unwrap(),
        };
        assert!(crate::layers::layers(&read).is_empty());
        assert!(crate::notes::notes(&read).is_empty());
        assert!(find_by_name(&read, "Wheel").is_some());
    }
    assert_eq!(crate::layers::own_layer(&dom, model), Some(layer));
    assert_eq!(crate::notes::notes(&dom).len(), 1);
}

/// File › Export as glTF… starts from `Workspace`, the root Studio's own
/// whole-place export has, and nests the model under it.
#[test]
fn export_as_gltf_of_the_place_is_rooted_at_workspace() {
    let (mut dom, _, wheel) = place();
    let outside = find_by_name(&dom, "Outside").unwrap();
    for part in [wheel, outside] {
        dom.set_property(
            part,
            "CFrame",
            Variant::CFrame(rbx_dom::CFrameData {
                position: rbx_dom::Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        )
        .unwrap();
        dom.set_property(
            part,
            "size",
            Variant::Vector3(rbx_dom::Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        )
        .unwrap();
    }
    let root = workspace(&dom).unwrap();

    let files = encode(
        Export::Gltf,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[root],
        Path::new("Place.gltf"),
    )
    .unwrap();
    let document: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    // The buffer is a file beside the document, which names it.
    assert_eq!(files[0].0, Path::new("Place.gltf"));
    assert_eq!(files[1].0, Path::new("Place.bin"));
    assert_eq!(document["buffers"][0]["uri"], "Place.bin");
    assert_eq!(document["buffers"][0]["byteLength"], files[1].1.len());

    let roots = document["scenes"][0]["nodes"].as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(
        document["nodes"][roots[0].as_u64().unwrap() as usize]["name"],
        "Workspace"
    );
    let names: Vec<_> = document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Car") && names.contains(&"Wheel") && names.contains(&"Outside"));
}

/// Export as FBX… writes one binary file with nothing beside it.
#[test]
fn export_as_fbx_is_one_binary_file() {
    let (mut dom, _, wheel) = place();
    dom.set_property(
        wheel,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: rbx_dom::Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
    dom.set_property(
        wheel,
        "size",
        Variant::Vector3(rbx_dom::Vector3Data {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }),
    )
    .unwrap();
    let files = encode(
        Export::Fbx,
        &dom,
        &database(),
        &pick::Meshes::default(),
        &[wheel],
        Path::new("Wheel.fbx"),
    )
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, Path::new("Wheel.fbx"));
    assert!(files[0].1.starts_with(b"Kaydara FBX Binary  \0"));
}
