use rbx_dom::{CFrameData, Content, Ref, Variant, Vector3Data, WeakDom};
use rbx_ugc::{AssetType, CageKind, Rule};

use super::{mesh_key, plan, resolve, Key};
use crate::shell::ugc::ItemKind;

/// A `version 2.00` mesh of one triangle spanning a 4 stud cube.
fn mesh_bytes() -> Vec<u8> {
    let mut body = Vec::from(b"version 2.00\n".as_slice());
    body.extend_from_slice(&12u16.to_le_bytes());
    body.extend_from_slice(&[40, 12]);
    body.extend_from_slice(&3u32.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    for corner in [[-2.0f32; 3], [2.0; 3], [0.0; 3]] {
        for c in corner {
            body.extend_from_slice(&c.to_le_bytes());
        }
        body.extend(std::iter::repeat_n(0u8, 28));
    }
    for index in 0u32..3 {
        body.extend_from_slice(&index.to_le_bytes());
    }
    body
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::new_rgba8(width, height)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn fetch(key: &Key) -> Result<Vec<u8>, String> {
    match key {
        Key::Id(2) => Ok(png_bytes(64, 32)),
        Key::Id(9) => Err("offline".into()),
        _ => Ok(mesh_bytes()),
    }
}

fn set(dom: &mut WeakDom, r: Ref, key: &str, value: Variant) {
    dom.set_property(r, key, value).unwrap();
}

fn uri(id: u64) -> Variant {
    Variant::Content(Content::Uri(format!("rbxassetid://{id}")))
}

/// Sized the way a place file stores it: `size`, not `Size`.
fn mesh_part(dom: &mut WeakDom, name: &str, parent: Ref, mesh: u64) -> Ref {
    let part = dom.new_instance("MeshPart", name, Some(parent));
    set(dom, part, "MeshId", uri(mesh));
    set(
        dom,
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    );
    part
}

fn kind(asset: AssetType) -> ItemKind {
    ItemKind {
        asset,
        ..ItemKind::default()
    }
}

#[test]
fn an_accessory_reads_its_handle_attachment_and_texture() {
    let mut dom = WeakDom::new();
    let acc = dom.new_instance("Accessory", "Cap", None);
    let handle = mesh_part(&mut dom, "Handle", acc, 1);
    let att = dom.new_instance("Attachment", "HatAttachment", Some(handle));
    set(
        &mut dom,
        att,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    let sa = dom.new_instance("SurfaceAppearance", "Skin", Some(handle));
    set(
        &mut dom,
        sa,
        "ColorMap",
        Variant::String("rbxassetid://2".into()),
    );

    let p = plan(&dom, acc, ItemKind::default()).unwrap();
    let got = resolve(&p, &fetch);
    assert_eq!(got.item.name, "Cap");
    let part = &got.item.parts[0];
    assert_eq!(part.attachments[0].name, "HatAttachment");
    assert_eq!(part.attachments[0].cframe[1], 1.0);
    let geo = part.geometry.as_ref().unwrap();
    assert!(geo.in_studs);
    assert_eq!(geo.positions[0], [-1.0; 3]);
    assert_eq!(geo.positions[1], [1.0; 3]);
    let tex = &got.item.textures[0];
    assert_eq!(
        (tex.owner.as_str(), tex.slot.as_str()),
        ("Skin", "ColorMap")
    );
    assert_eq!((tex.width, tex.height), (64, 32));
    assert!(!got.emissive);
}

#[test]
fn a_body_model_keeps_only_the_r15_parts() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Body", None);
    mesh_part(&mut dom, "Head", model, 1);
    mesh_part(&mut dom, "UpperTorso", model, 1);
    mesh_part(&mut dom, "Prop", model, 1);
    let p = plan(&dom, model, kind(AssetType::Body)).unwrap();
    let names: Vec<_> = p.parts.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Head", "UpperTorso"]);
    assert_eq!(p.name, "Body");
}

#[test]
fn a_wrap_layer_gives_an_inner_and_an_outer_cage() {
    let mut dom = WeakDom::new();
    let acc = dom.new_instance("Accessory", "Jacket", None);
    let handle = mesh_part(&mut dom, "Handle", acc, 1);
    let wrap = dom.new_instance("WrapLayer", "Wrap", Some(handle));
    set(&mut dom, wrap, "ReferenceMeshId", uri(3));
    set(&mut dom, wrap, "CageMeshId", uri(4));
    let got = resolve(&plan(&dom, acc, ItemKind::default()).unwrap(), &fetch);
    let cages = &got.item.cages;
    assert_eq!(cages.len(), 2);
    assert_eq!(cages[0].kind, CageKind::Inner);
    assert_eq!(cages[0].name, "Jacket_InnerCage");
    assert_eq!(cages[1].name, "Jacket_OuterCage");
    assert_eq!(cages[1].template.as_ref(), Some(&cages[0].geometry));
}

#[test]
fn cage_named_mesh_parts_in_a_model_become_cages() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Vest", None);
    mesh_part(&mut dom, "Vest", model, 1);
    mesh_part(&mut dom, "Vest_InnerCage", model, 3);
    let p = plan(&dom, model, ItemKind::default()).unwrap();
    assert_eq!(p.parts.len(), 1);
    assert_eq!(p.cages.len(), 1);
}

#[test]
fn an_unreadable_texture_becomes_a_note() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("MeshPart", "Rock", None);
    set(
        &mut dom,
        part,
        "TextureID",
        Variant::String("rbxassetid://9".into()),
    );
    let got = resolve(&plan(&dom, part, ItemKind::default()).unwrap(), &fetch);
    let unreadable: Vec<_> = got
        .notes
        .iter()
        .filter(|n| n.rule == Rule::Unreadable)
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].subject, "Rock.TextureID");
    assert!(got.item.parts[0].geometry.is_none());
}

#[test]
fn other_classes_are_refused() {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "F", None);
    assert_eq!(
        plan(&dom, folder, ItemKind::default()).unwrap_err(),
        "Pick an Accessory, a MeshPart or a Model"
    );
}

#[test]
fn a_local_mesh_is_read_from_the_cache_path() {
    assert_eq!(
        mesh_key("rbxasset://rbx-native-local/fitting/ab/Vest_InnerCage.mesh"),
        Some(Key::Local(
            "rbx-native-local/fitting/ab/Vest_InnerCage.mesh".into()
        ))
    );
    assert_eq!(mesh_key("rbxassetid://7"), Some(Key::Id(7)));
    assert_eq!(mesh_key("rbxasset://fonts/x.mesh"), None);
}

#[test]
fn the_tree_gets_studios_schema_rules() {
    let mut dom = WeakDom::new();
    let acc = dom.new_instance("Accessory", "Cap", None);
    let handle = mesh_part(&mut dom, "Handle", acc, 1);
    dom.new_instance("Attachment", "HatAttachment", Some(handle));
    dom.new_instance("Part", "Junk", Some(handle));
    set(&mut dom, handle, "Anchored", Variant::Bool(true));
    set(&mut dom, handle, "Tags", Variant::String("Shiny".into()));
    let p = plan(&dom, acc, kind(AssetType::Hat)).unwrap();
    let messages: Vec<_> = p.notes.iter().map(|f| f.message.as_str()).collect();
    for wanted in [
        "The following instances are not part of the approved schema and should be removed: Cap.Handle.Junk.",
        "Property 'Anchored' on 'Cap.Handle' is invalid. Expected to be = false.",
        "The following instances have tags but marketplace assets cannot have any tags: Cap.Handle",
    ] {
        assert!(messages.contains(&wanted), "{messages:?}");
    }
}

#[test]
fn a_renamed_handle_is_named_like_studio() {
    let mut dom = WeakDom::new();
    let acc = dom.new_instance("Accessory", "Cap", None);
    mesh_part(&mut dom, "Brim", acc, 1);
    let p = plan(&dom, acc, kind(AssetType::Hat)).unwrap();
    assert!(p.notes.iter().any(|f| f.rule == Rule::Schema
        && f.message == "Could not find required MeshPart called Handle inside of Cap"));
}
