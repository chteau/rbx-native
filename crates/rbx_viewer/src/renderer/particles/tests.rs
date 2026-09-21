use super::*;
use crate::quality::QualityLevel;
use glam::Mat4;
use rbx_dom::{ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref};

fn texture(id: u64) -> rbx_assets::AssetRef {
    rbx_assets::AssetRef::Id(id)
}

// A minimal emitter for a wiring test that only cares about `texture`
// (and, in `super::patch`'s tests, `referent`).
pub(super) fn emitter(texture: rbx_assets::AssetRef) -> Emitter {
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
        time_scale: 1.0,
        cap: 0,
        seed: 1,
        referent: Ref::new(1),
        slot: 0,
        volume: Mat4::IDENTITY,
    }
}

// A quality level switched between two scenes has to take on the next
// rebuild: whether particles draw is read from the profile every rebuild,
// not only once when the pass was built.
#[test]
fn a_rebuild_follows_the_quality_toggle_it_is_given() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    let target = super::super::pipeline::Target {
        format: crate::renderer::post::HDR_FORMAT,
        samples: 1,
    };
    let mut on = QualityLevel::Automatic.profile();
    on.particles = true;
    let mut off = on;
    off.particles = false;

    let images = Answered::default();

    let mut pass = Particles::new(&device, &queue, target, &[], &images, &off);
    assert!(!pass.enabled);

    pass.rebuild(&device, &queue, &[], &images, &on);
    assert!(pass.enabled);

    pass.rebuild(&device, &queue, &[], &images, &off);
    assert!(!pass.enabled);
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

// A `ParticleEmitter` with no texture at all draws through the built-in
// white slot and has nothing to ask the loader for. Leaving `Empty` in the
// list would leave it untried for the life of the session — see
// `renderer::rebuild::untried` — and re-evaluated on every rebuild.
#[test]
fn a_textureless_emitter_asks_for_nothing() {
    let emitters = [emitter(AssetRef::Empty), emitter(texture(1))];
    assert_eq!(texture_refs(&emitters), vec![texture(1)]);
}
