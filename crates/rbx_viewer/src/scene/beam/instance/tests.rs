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

/// A DOM with two parts, each holding one `Attachment`, and a `Beam` linking
/// them — the minimal shape [`plan`] walks.
fn fixture(properties: Vec<(&str, Variant)>) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();

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

    let beam = Ref::new(5);
    let mut instance = Instance::new(beam, "Beam", "Beam");
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
    dom.set_parent(beam, None);

    (dom, beam)
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

#[test]
fn a_beam_with_no_properties_falls_back_to_documented_defaults() {
    let (dom, _) = fixture(vec![]);
    let beams = plan(&dom, &database());
    assert_eq!(beams.len(), 1);
    let beam = &beams[0];
    assert_eq!(beam.width0, DEFAULT_WIDTH);
    assert_eq!(beam.width1, DEFAULT_WIDTH);
    assert_eq!(beam.texture_length, DEFAULT_TEXTURE_LENGTH);
    assert_eq!(beam.texture_speed, DEFAULT_TEXTURE_SPEED);
    assert_eq!(beam.segments, DEFAULT_SEGMENTS);
    assert_eq!(
        beam.texture,
        AssetRef::Empty,
        "no Texture set means flat colour"
    );
    assert!(!beam.face_camera);
    assert!((beam.curve.position(0.0) - Vec3::ZERO).length() < 1e-5);
    assert!((beam.curve.position(1.0) - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-5);
}

#[test]
fn a_disabled_beam_builds_nothing() {
    let (dom, _) = fixture(vec![("Enabled", Variant::Bool(false))]);
    assert!(plan(&dom, &database()).is_empty());
}

#[test]
fn a_dangling_attachment_ref_builds_nothing() {
    let mut dom = WeakDom::new();
    let beam = Ref::new(1);
    let mut instance = Instance::new(beam, "Beam", "Beam");
    instance
        .properties_mut()
        .insert("Attachment0".to_string(), Variant::Ref(Ref::new(999)));
    instance
        .properties_mut()
        .insert("Attachment1".to_string(), Variant::Ref(Ref::new(998)));
    dom.insert(instance);
    dom.set_parent(beam, None);

    assert!(plan(&dom, &database()).is_empty());
}

#[test]
fn texture_mode_enum_ordinals_map_stretch_and_wrap_static_alike() {
    let (dom, _) = fixture(vec![("TextureMode", Variant::Enum(2))]);
    let beams = plan(&dom, &database());
    assert_eq!(
        beams[0].texture_mode,
        TextureMode::Wrap,
        "Static folds into Wrap"
    );
}

#[test]
fn a_custom_color_sequence_is_kept_verbatim() {
    let sequence = ColorSequence {
        keypoints: vec![
            ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                envelope: 0.0,
            },
            ColorSequenceKeypoint {
                time: 1.0,
                color: Color3Data {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                },
                envelope: 0.0,
            },
        ],
    };
    let (dom, _) = fixture(vec![("Color", Variant::ColorSequence(sequence.clone()))]);
    let beams = plan(&dom, &database());
    assert_eq!(beams[0].color, sequence);
}
