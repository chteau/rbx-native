use super::*;
use crate::{BodyScale, Geometry, Part, Placement, IDENTITY};

fn hat(half: f32, in_studs: bool) -> Item {
    Item {
        name: "Hat".into(),
        asset_type: AssetType::Hat,
        scale: BodyScale::Normal,
        parts: vec![Part {
            name: "Handle".into(),
            geometry: Some(Geometry {
                positions: vec![[half, 0.0, 0.0], [-half, 0.0, 0.0]],
                bones: Vec::new(),
                in_studs,
                ..Default::default()
            }),
            cframe: IDENTITY,
            attachments: vec![Placement {
                name: "HatAttachment".into(),
                cframe: IDENTITY,
            }],
        }],
        textures: vec![],
        cages: vec![],
    }
}

#[test]
fn a_hat_inside_its_box_has_no_findings() {
    let mut out = Vec::new();
    size(&hat(0.5, true), &mut out);
    assert!(out.is_empty());
}

#[test]
fn a_hat_wider_than_its_box_is_an_error() {
    let mut out = Vec::new();
    size(&hat(2.0, true), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].severity, Severity::Error);
    assert_eq!(out[0].rule, Rule::ItemSize);
}

#[test]
fn a_mesh_in_unknown_units_is_only_a_note() {
    let mut out = Vec::new();
    size(&hat(2.0, false), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].severity, Severity::Note);
}

fn limb(name: &str, x: f32) -> Part {
    let mut cframe = IDENTITY;
    cframe[0] = x;
    Part {
        name: name.into(),
        geometry: Some(Geometry {
            positions: vec![[-0.5, -1.0, -0.5], [0.5, 1.0, 0.5]],
            bones: Vec::new(),
            in_studs: true,
            ..Default::default()
        }),
        cframe,
        attachments: vec![],
    }
}

#[test]
fn each_arm_is_measured_on_its_own() {
    let item = Item {
        name: "Body".into(),
        asset_type: AssetType::Body,
        scale: BodyScale::Normal,
        parts: vec![
            limb("LeftUpperArm", -3.0),
            limb("RightUpperArm", 3.0),
            limb("UpperTorso", 0.0),
        ],
        textures: vec![],
        cages: vec![],
    };
    let mut out = Vec::new();
    size(&item, &mut out);
    assert!(out.iter().all(|f| f.rule != Rule::BodyPartSize), "{out:?}");
}

#[test]
fn an_arm_longer_than_its_limit_is_named() {
    let mut arm = limb("LeftUpperArm", 0.0);
    arm.geometry.as_mut().unwrap().positions = vec![[0.0, -3.0, 0.0], [0.5, 3.0, 0.5]];
    let item = Item {
        name: "Body".into(),
        asset_type: AssetType::DynamicHead,
        scale: BodyScale::Normal,
        parts: vec![arm],
        textures: vec![],
        cages: vec![],
    };
    let mut out = Vec::new();
    size(&item, &mut out);
    assert!(out.is_empty(), "a head check ignores arms: {out:?}");
    let body = Item {
        asset_type: AssetType::Body,
        ..item
    };
    size(&body, &mut out);
    assert_eq!(out[0].subject, "LeftArm");
}
