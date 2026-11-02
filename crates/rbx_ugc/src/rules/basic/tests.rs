use super::*;
use crate::{BodyScale, Geometry, Part, Placement, Texture, IDENTITY};

fn part(name: &str, tris: usize) -> Part {
    Part {
        name: name.to_string(),
        geometry: Some(Geometry {
            triangles: vec![[0, 0, 0]; tris],
            ..Default::default()
        }),
        cframe: IDENTITY,
        attachments: Vec::new(),
    }
}

fn item(asset_type: AssetType, parts: Vec<Part>) -> Item {
    Item {
        name: "Thing".to_string(),
        asset_type,
        scale: BodyScale::Normal,
        parts,
        textures: Vec::new(),
        cages: Vec::new(),
    }
}

#[test]
fn an_unreadable_part_is_a_note() {
    let mut p = part("Handle", 1);
    p.geometry = None;
    let mut out = Vec::new();
    unreadable(&item(AssetType::Hat, vec![p]), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].severity, Severity::Note);
}

#[test]
fn a_hat_over_4k_triangles_is_an_error() {
    let mut out = Vec::new();
    triangles(&item(AssetType::Hat, vec![part("Handle", 4001)]), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].rule, Rule::AccessoryTriangles);
}

#[test]
fn two_parts_in_an_accessory_break_the_single_mesh_rule() {
    let mut out = Vec::new();
    let it = item(AssetType::Hat, vec![part("A", 1), part("B", 1)]);
    triangles(&it, &mut out);
    assert!(out.iter().any(|f| f.rule == Rule::SingleMesh));
}

#[test]
fn a_dynamic_head_only_checks_its_own_row() {
    let mut out = Vec::new();
    let it = item(
        AssetType::DynamicHead,
        vec![part("Head", 4001), part("UpperTorso", 5000)],
    );
    triangles(&it, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].subject, "DynamicHead");
}

#[test]
fn an_oversized_texture_is_an_error() {
    let mut it = item(AssetType::Hat, vec![]);
    it.textures.push(Texture {
        owner: "Handle".to_string(),
        slot: "TextureID".to_string(),
        width: 4096,
        height: 512,
    });
    let mut out = Vec::new();
    textures(&it, &mut out);
    assert_eq!(out[0].subject, "Handle.TextureID");
}

#[test]
fn five_influences_on_a_vertex_is_an_error() {
    let mut p = part("Handle", 1);
    p.geometry.as_mut().unwrap().influences = vec![4, 5, 6];
    let mut out = Vec::new();
    influences(&item(AssetType::Hat, vec![p]), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].measured, Some(Value::Count(6)));
}

#[test]
fn a_hat_without_its_attachment_is_an_error() {
    let mut out = Vec::new();
    attachment(&item(AssetType::Hat, vec![part("Handle", 1)]), &mut out);
    assert_eq!(out.len(), 1);
    let mut p = part("Handle", 1);
    p.attachments.push(Placement {
        name: "HatAttachment".to_string(),
        cframe: IDENTITY,
    });
    let mut out = Vec::new();
    attachment(&item(AssetType::Hat, vec![p]), &mut out);
    assert!(out.is_empty());
}

#[test]
fn eyebrows_need_both_face_joints() {
    let mut p = part("Handle", 1);
    let found = |p: &Part| {
        let mut out = Vec::new();
        joints(&item(AssetType::Eyebrow, vec![p.clone()]), &mut out);
        out.into_iter().map(|f| f.message).collect::<Vec<_>>()
    };
    assert_eq!(
        found(&p),
        [
            "EyebrowAccessory must use skinning transfer, but you are missing the required joint RBX_Leader.",
            "EyebrowAccessory must use skinning transfer, but you are missing the required joint RBX_Follower.",
        ]
    );
    p.geometry.as_mut().unwrap().bones = vec!["RBX_Leader".into(), "RBX_Follower".into()];
    assert!(found(&p).is_empty());
}
