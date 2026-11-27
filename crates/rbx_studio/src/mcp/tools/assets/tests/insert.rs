use rbx_dom::{Variant, Vector3Data, WeakDom};
use serde_json::{json, Value};

use super::super::super::finished;
use super::super::super::tests::place;
use super::super::insert::{build, Kind};
use super::super::toolbox_fetch;
use super::{deliver, insert, mock, model_bytes, MESH, PNG};
use crate::mcp::Insertion;

#[test]
fn a_model_lands_in_a_model_named_after_the_asset() {
    deliver(10, model_bytes());
    let dom = place();
    let insertion = insert(&dom, json!({ "assetId": "1818" })).unwrap();
    assert_eq!(insertion.asset_id, 1818);
    assert_eq!(insertion.asset_type, "Model");
    assert_eq!(insertion.asset_name, "Rusty Crate");
    assert_eq!(dom.get(insertion.parent).unwrap().class(), "Workspace");
    let content = &insertion.content;
    let [wrapper] = content.root_refs() else {
        panic!("one root expected");
    };
    let wrapper = content.get(*wrapper).unwrap();
    assert_eq!((wrapper.class(), wrapper.name()), ("Model", "Rusty Crate"));
    let crate_model = content.get(wrapper.children()[0]).unwrap();
    assert_eq!(
        (crate_model.class(), crate_model.name()),
        ("Model", "Crate")
    );
}

#[test]
fn an_animation_lands_as_its_keyframe_sequence_named_after_the_asset() {
    let mut dom = WeakDom::new();
    dom.new_instance("KeyframeSequence", "Dance", None);
    let bytes = rbx_binary::serialize(&dom).unwrap();
    let content = build(5, Kind::Animation, &bytes, "R15Dance").unwrap();
    let only = content.get(content.root_refs()[0]).unwrap();
    assert_eq!(
        (only.class(), only.name()),
        ("KeyframeSequence", "R15Dance")
    );
}

#[test]
fn raw_assets_become_the_instance_that_plays_them() {
    let cases: [(u32, Vec<u8>, &str, &str, &str); 3] = [
        (13, PNG.to_vec(), "Decal", "Decal", "Texture"),
        (3, b"OggS\0\x02".to_vec(), "Audio", "Sound", "SoundId"),
        (
            62,
            b"\x1aE\xdf\xa3webm".to_vec(),
            "Video",
            "VideoFrame",
            "Video",
        ),
    ];
    for (type_id, bytes, asset_type, class, property) in cases {
        deliver(type_id, bytes);
        let insertion = insert(&place(), json!({ "assetId": 1818 })).unwrap();
        assert_eq!(insertion.asset_type, asset_type);
        let content = &insertion.content;
        let made = content.get(content.root_refs()[0]).unwrap();
        assert_eq!(made.class(), class);
        assert_eq!(made.name(), "Rusty Crate", "{class} takes the asset's name");
        let value = made.properties().get(property).unwrap_or_else(|| {
            panic!(
                "{class}.{property} unset: {:?}",
                made.properties().keys().collect::<Vec<_>>()
            )
        });
        assert!(
            format!("{value:?}").contains("rbxassetid://1818"),
            "{value:?}"
        );
    }
}

#[test]
fn a_hint_and_a_name_are_taken_as_given() {
    // Delivery reports no type it knows, so the hint decides.
    deliver(0, b"OggS\0\x02".to_vec());
    let insertion = insert(
        &place(),
        json!({ "assetId": "1818", "assetType": "Audio", "assetName": "Tacos" }),
    )
    .unwrap();
    assert_eq!(insertion.asset_type, "Audio");
    let made = insertion
        .content
        .get(insertion.content.root_refs()[0])
        .unwrap();
    assert_eq!((made.class(), made.name()), ("Sound", "Tacos"));
}

#[test]
fn the_type_delivery_reports_wins_over_the_hint_and_tags_the_insert() {
    deliver(13, PNG.to_vec());
    for hint in ["Image", "Hat"] {
        let insertion = insert(&place(), json!({ "assetId": "1818", "assetType": hint })).unwrap();
        assert_eq!(insertion.asset_type, "Decal");
        let made = insertion
            .content
            .get(insertion.content.root_refs()[0])
            .unwrap();
        assert_eq!(made.class(), "Decal");
        assert_eq!(made.tags(), [format!("Assistant:{}", insertion.guid)]);
    }
}

#[test]
fn a_mesh_becomes_an_anchored_mesh_part_of_its_own_size() {
    let content = build(5, Kind::Mesh, MESH, "Rock").unwrap();
    let part = content.get(content.root_refs()[0]).unwrap();
    assert_eq!((part.class(), part.name()), ("MeshPart", "Rock"));
    let six = Variant::Vector3(v3(4.0, 2.0, 6.0));
    assert_eq!(part.properties().get("size"), Some(&six));
    assert_eq!(part.properties().get("InitialSize"), Some(&six));
    assert!(!part.properties().contains_key("Size"));
    assert_eq!(
        part.properties().get("Anchored"),
        Some(&Variant::Bool(true))
    );
    assert_eq!(Kind::sniff(MESH), Some(Kind::Mesh));
}

fn v3(x: f32, y: f32, z: f32) -> Vector3Data {
    Vector3Data { x, y, z }
}

/// A `version 2.00` mesh whose corners are `min` and `max`.
fn mesh_spanning(min: [f32; 3], max: [f32; 3]) -> Vec<u8> {
    let mut body = Vec::from(b"version 2.00\n".as_slice());
    body.extend_from_slice(&12u16.to_le_bytes());
    body.extend_from_slice(&[36, 12]);
    body.extend_from_slice(&3u32.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    for corner in [min, max, [0.0; 3]] {
        for c in corner {
            body.extend_from_slice(&c.to_le_bytes());
        }
        body.extend(std::iter::repeat_n(0u8, 24));
    }
    for index in 0u32..3 {
        body.extend_from_slice(&index.to_le_bytes());
    }
    body
}

/// The corners of three Toolbox meshes (the gun of asset 430286487, the
/// hair of 454910984 and of 431984629), and the `Size` and `InitialSize`
/// Studio gave the MeshPart `AssetService:CreateMeshPartAsync` made from
/// each.
const STUDIO_MESH_SIZES: [([f32; 3], [f32; 3], [f32; 3]); 3] = [
    (
        [-23.104_46, -12.098_463, -4.338_474_3],
        [23.104_46, 12.098_461, 4.338_474_3],
        [46.208_92, 24.196_924, 8.676_949],
    ),
    (
        [-0.109_569_5, -0.246_325_02, -0.136_799],
        [0.109_569_5, 0.246_325_02, 0.136_799],
        [0.219_139, 0.492_650_03, 0.273_598],
    ),
    (
        [-174.679_98, -388.253_9, -254.874_94],
        [174.679_98, 388.253_9, 254.874_94],
        [349.359_95, 776.507_8, 509.749_88],
    ),
];

#[test]
fn a_mesh_part_is_as_big_as_studio_makes_it_from_the_same_mesh() {
    for (min, max, [x, y, z]) in STUDIO_MESH_SIZES {
        let content = build(5, Kind::Mesh, &mesh_spanning(min, max), "Mesh").unwrap();
        let part = content.get(content.root_refs()[0]).unwrap();
        let studio = Variant::Vector3(v3(x, y, z));
        assert_eq!(part.properties().get("size"), Some(&studio));
        assert_eq!(part.properties().get("InitialSize"), Some(&studio));
    }
}

#[test]
fn a_mesh_past_2048_studs_is_refused_as_studio_refuses_it() {
    // The house mesh of asset 437409334, 2589.7 studs deep.
    let house = mesh_spanning(
        [-630.000_06, -668.282_6, -1_294.861_4],
        [630.000_06, 668.282_5, 1_294.861_4],
    );
    let err = build(5, Kind::Mesh, &house, "House").unwrap_err();
    assert_eq!(
        err,
        "asset 5: Mesh extents size too large. Maximum is (2048, 2048, 2048)."
    );
}

#[test]
fn a_toolbox_mesh_part_keeps_the_size_its_asset_gives_it() {
    // What Studio's Toolbox gave the same house as a MeshPart asset: the
    // asset's own sizes, its 2589.7-stud depth held to 2048 only in `size`.
    let size = v3(1_260.000_1, 1_336.565_1, 2_048.0);
    let initial = v3(1_260.000_1, 1_336.565_1, 2_589.723);
    let mut asset = WeakDom::new();
    let part = asset.new_instance("MeshPart", "MeshPart", None);
    asset.set_property(part, "size", Variant::Vector3(size)).unwrap();
    asset
        .set_property(part, "InitialSize", Variant::Vector3(initial))
        .unwrap();
    deliver(40, rbx_binary::serialize(&asset).unwrap());
    let (content, _) = toolbox_fetch(1818).unwrap();
    let [root] = content.root_refs() else {
        panic!("the MeshPart itself expected, unwrapped");
    };
    let part = content.get(*root).unwrap();
    assert_eq!(part.class(), "MeshPart");
    assert_eq!(part.properties().get("size"), Some(&Variant::Vector3(size)));
    assert_eq!(
        part.properties().get("InitialSize"),
        Some(&Variant::Vector3(initial))
    );
}

#[test]
fn an_unresolved_parent_falls_back_to_workspace_with_studios_warning() {
    deliver(10, model_bytes());
    let dom = place();
    let insertion = insert(
        &dom,
        json!({ "assetId": "1818", "parentPath": "game.Workspace.Nope" }),
    )
    .unwrap();
    assert_eq!(dom.get(insertion.parent).unwrap().class(), "Workspace");
    assert_eq!(
        insertion.parent_warning.as_deref(),
        Some("Could not resolve path \"game.Workspace.Nope\": \"Nope\" not found. Defaulting to workspace.")
    );
    let insertion = insert(
        &dom,
        json!({ "assetId": "1818", "parentPath": "game.Workspace.Tower" }),
    )
    .unwrap();
    assert_eq!(dom.get(insertion.parent).unwrap().name(), "Tower");
    assert!(insertion.parent_warning.is_none());
}

#[test]
fn an_insert_it_cannot_do_answers_studios_error() {
    let dom = place();
    let err = |arguments| -> Value {
        serde_json::from_str(&insert(&dom, arguments).err().unwrap()).unwrap()
    };
    assert_eq!(
        err(json!({ "assetId": "abc" })),
        json!({ "status": "error", "error": "Invalid asset ID: abc" })
    );
    deliver(9, b"<roblox>".to_vec());
    assert!(err(json!({ "assetId": "1818" }))["error"]
        .as_str()
        .unwrap()
        .contains("is a place"));
    mock(|target, _| {
        if target.contains("/asset-delivery-api/") {
            (
                403,
                br#"{"errors":[{"message":"User is not authorized to access Asset."}]}"#.to_vec(),
            )
        } else {
            (401, Vec::new())
        }
    });
    let refused = err(json!({ "assetId": "1818" }));
    assert!(
        refused["error"].as_str().unwrap().contains("1818"),
        "{refused}"
    );
}

#[test]
fn a_finished_insert_answers_as_studio_does() {
    let mut dom = place();
    let workspace = dom.root_refs()[0];
    let crate_ref = dom.new_instance("Model", "Crate", Some(workspace));
    let insertion = Insertion {
        asset_id: 1818,
        parent: workspace,
        content: WeakDom::new(),
        asset_type: "Model".to_owned(),
        asset_name: "Crate".to_owned(),
        is_package: false,
        parent_warning: None,
        guid: "g-1".to_owned(),
    };
    let answer = finished(Ok(super::super::inserted(&dom, &[crate_ref], &insertion)));
    let text: Value = serde_json::from_str(answer["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        text,
        json!({
            "status": "success",
            "insertGuid": "g-1",
            "tag": "Assistant:g-1",
            "assetId": "1818",
            "assetType": "Model",
            "assetName": "Crate",
            "isPackage": false,
            "sandboxed": false,
            "insertedInstances": [{ "fullPath": "game.Workspace.Crate", "name": "Crate", "className": "Model" }],
        })
    );
}

#[test]
fn an_old_models_brick_colours_become_colours_a_place_can_save() {
    let model = br#"<roblox version="4">
<Item class="Model" referent="RBX0"><Properties><string name="Name">Old</string></Properties>
<Item class="Part" referent="RBX1"><Properties><string name="Name">Red</string><int name="BrickColor">21</int></Properties></Item>
<Item class="Part" referent="RBX2"><Properties><string name="Name">Blue</string><Color3uint8 name="Color3uint8">4278190335</Color3uint8></Properties></Item>
</Item></roblox>"#;
    let content = build(7, Kind::Model, model, "Old").unwrap();
    // The wrapper named after the asset, then the file's own Model.
    let wrapper = content.get(content.root_refs()[0]).unwrap();
    let old = content.get(wrapper.children()[0]).unwrap();
    let mut parts: Vec<_> = old
        .children()
        .iter()
        .filter_map(|&r| content.get(r))
        .collect();
    parts.sort_by_key(|part| part.name().to_owned());
    assert!(parts
        .iter()
        .all(|part| !part.properties().contains_key("BrickColor")));
    assert_eq!(
        parts[1].properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 196,
            g: 40,
            b: 28
        })
    );
    crate::save::Format::Binary.encode(&content).unwrap();
}
