use glam::Vec3;
use rbx_dom::{CFrameData, Color3Data, ColorSequenceKeypoint, Instance, Vector3Data};

use super::*;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn cframe(position: Vec3) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation: IDENTITY_ROTATION,
    })
}

/// A DOM with two parts, each holding one `Attachment`, and a `Trail` linking
/// them — the minimal shape [`plan`] walks, mirroring
/// `scene::beam::instance::tests::fixture`.
fn fixture(properties: Vec<(&str, Variant)>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();

    // `plan` only looks for a `Trail` under `Workspace` now (see
    // `super::plan`'s doc comment) — the parts/attachments themselves stay at
    // root since `ParentMap` (from `scene::beam`) still walks the whole DOM.
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);

    let part0 = Ref::new(1);
    let mut instance = Instance::new(part0, "Part", "Part");
    instance
        .properties_mut()
        .insert("CFrame".to_string(), cframe(Vec3::ZERO));
    dom.insert(instance);
    dom.set_parent(part0, None);

    let attachment0 = Ref::new(2);
    let mut instance = Instance::new(attachment0, "Attachment", "Attachment");
    instance
        .properties_mut()
        .insert("CFrame".to_string(), cframe(Vec3::ZERO));
    dom.insert(instance);
    dom.set_parent(attachment0, Some(part0));

    let part1 = Ref::new(3);
    let mut instance = Instance::new(part1, "Part", "Part");
    instance
        .properties_mut()
        .insert("CFrame".to_string(), cframe(Vec3::new(10.0, 0.0, 0.0)));
    dom.insert(instance);
    dom.set_parent(part1, None);

    let attachment1 = Ref::new(4);
    let mut instance = Instance::new(attachment1, "Attachment", "Attachment");
    instance
        .properties_mut()
        .insert("CFrame".to_string(), cframe(Vec3::ZERO));
    dom.insert(instance);
    dom.set_parent(attachment1, Some(part1));

    let trail = Ref::new(5);
    let mut instance = Instance::new(trail, "Trail", "Trail");
    instance
        .properties_mut()
        .insert("Attachment0".to_string(), Variant::Ref(attachment0));
    instance
        .properties_mut()
        .insert("Attachment1".to_string(), Variant::Ref(attachment1));
    for (key, value) in properties {
        instance.properties_mut().insert(key.to_string(), value);
    }
    dom.insert(instance);
    dom.set_parent(trail, Some(workspace));

    (dom, trail)
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_trail_with_no_properties_falls_back_to_documented_defaults() {
    let (dom, _) = fixture(vec![]);
    let trails = plan(&dom, &database());
    assert_eq!(trails.len(), 1);
    let trail = &trails[0];
    assert!(trail.enabled);
    assert_eq!(trail.lifetime, DEFAULT_LIFETIME);
    assert_eq!(trail.min_length, DEFAULT_MIN_LENGTH);
    assert_eq!(trail.texture_length, DEFAULT_TEXTURE_LENGTH);
    assert_eq!(trail.texture, AssetRef::Empty);
    assert_eq!(trail.light_emission, 0.0);
    assert_eq!(trail.position0, Vec3::ZERO);
    assert_eq!(trail.position1, Vec3::new(10.0, 0.0, 0.0));
}

#[test]
fn width_scale_defaults_to_a_flat_one() {
    let (dom, _) = fixture(vec![]);
    let trails = plan(&dom, &database());
    let trail = &trails[0];
    assert_eq!(trail.width_scale.keypoints.len(), 2);
    assert!(trail
        .width_scale
        .keypoints
        .iter()
        .all(|keypoint| keypoint.value == 1.0));
}

#[test]
fn color_and_transparency_default_to_white_and_opaque() {
    let (dom, _) = fixture(vec![]);
    let trails = plan(&dom, &database());
    let trail = &trails[0];
    for keypoint in &trail.color.keypoints {
        assert_eq!(
            keypoint.color,
            Color3Data {
                r: 1.0,
                g: 1.0,
                b: 1.0
            }
        );
    }
    for keypoint in &trail.transparency.keypoints {
        assert_eq!(keypoint.value, 0.0);
    }
}

#[test]
fn explicit_properties_override_every_default() {
    let (dom, _) = fixture(vec![
        ("Enabled", Variant::Bool(false)),
        ("Lifetime", Variant::Float32(5.0)),
        ("MinLength", Variant::Float32(2.0)),
        ("TextureLength", Variant::Float32(3.0)),
        ("LightEmission", Variant::Float32(0.5)),
        (
            "WidthScale",
            Variant::NumberSequence(NumberSequence {
                keypoints: vec![NumberSequenceKeypoint {
                    time: 0.0,
                    value: 0.5,
                    envelope: 0.0,
                }],
            }),
        ),
        (
            "Color",
            Variant::ColorSequence(ColorSequence {
                keypoints: vec![ColorSequenceKeypoint {
                    time: 0.0,
                    color: Color3Data {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                    envelope: 0.0,
                }],
            }),
        ),
    ]);
    let trails = plan(&dom, &database());
    let trail = &trails[0];
    assert!(!trail.enabled);
    assert_eq!(trail.lifetime, 5.0);
    assert_eq!(trail.min_length, 2.0);
    assert_eq!(trail.texture_length, 3.0);
    assert_eq!(trail.light_emission, 0.5);
    assert_eq!(trail.width_scale.keypoints[0].value, 0.5);
    assert_eq!(trail.color.keypoints[0].color.r, 1.0);
}

#[test]
fn a_dangling_attachment_reference_drops_the_trail() {
    let (mut dom, trail) = fixture(vec![]);
    dom.get_mut(trail)
        .unwrap()
        .properties_mut()
        .insert("Attachment1".to_string(), Variant::Ref(Ref::new(999)));
    assert!(plan(&dom, &database()).is_empty());
}

#[test]
fn a_trail_with_no_attachment_properties_is_dropped() {
    let mut dom = WeakDom::new();
    let trail = Ref::new(1);
    dom.insert(Instance::new(trail, "Trail", "Trail"));
    dom.set_parent(trail, None);
    assert!(plan(&dom, &database()).is_empty());
}
