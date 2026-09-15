use super::*;
use glam::Mat4;
use rbx_dom::{ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint};

fn texture(id: u64) -> rbx_assets::AssetRef {
    rbx_assets::AssetRef::Id(id)
}

// A minimal emitter for a wiring test that only cares about `texture`.
fn emitter(texture: rbx_assets::AssetRef) -> Emitter {
    let flat = NumberSequence {
        keypoints: vec![NumberSequenceKeypoint {
            time: 0.0,
            value: 1.0,
            envelope: 0.0,
        }],
    };
    Emitter {
        rate: 0.0,
        lifetime: (1.0, 1.0),
        speed: (0.0, 0.0),
        spread_degrees: (0.0, 0.0),
        direction: Vec3::Y,
        acceleration: Vec3::ZERO,
        drag: 0.0,
        size: flat.clone(),
        transparency: flat,
        color: ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: rbx_dom::Color3Data {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
                envelope: 0.0,
            }],
        },
        texture,
        light_emission: 0.0,
        rotation_degrees: (0.0, 0.0),
        rot_speed_degrees: (0.0, 0.0),
        z_offset: 0.0,
        cap: 0,
        seed: 1,
        volume: Mat4::IDENTITY,
    }
}

#[test]
fn texture_refs_are_deduplicated_in_first_seen_order() {
    let emitters = [
        emitter(texture(1)),
        emitter(texture(2)),
        emitter(texture(1)),
    ];
    assert_eq!(texture_refs(&emitters), vec![texture(1), texture(2)]);
}
