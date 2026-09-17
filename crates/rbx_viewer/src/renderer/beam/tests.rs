use glam::Vec3;
use rbx_dom::{ColorSequence, NumberSequence};

use super::*;
use crate::quality::QualityLevel;
use crate::scene::{Curve, TextureMode};

/// A textureless beam: `AssetRef::Empty` draws through the built-in white
/// slot, so building it fetches nothing.
fn beam() -> Beam {
    Beam {
        curve: Curve::new(Vec3::ZERO, Vec3::X, 0.0, Vec3::X * 4.0, Vec3::X, 0.0),
        width0: 1.0,
        width1: 1.0,
        color: ColorSequence {
            keypoints: Vec::new(),
        },
        transparency: NumberSequence {
            keypoints: Vec::new(),
        },
        texture: AssetRef::Empty,
        texture_length: 1.0,
        texture_mode: TextureMode::Stretch,
        texture_speed: 0.0,
        light_emission: 0.0,
        light_influence: 1.0,
        face_camera: true,
        secondary_axis0: Vec3::Y,
        secondary_axis1: Vec3::Y,
        segments: 10,
        z_offset: 0.0,
    }
}

// A quality level switched between two scenes has to take on the next
// rebuild: whether beams draw is read from the profile every rebuild, not
// only once when the pass was built.
#[test]
fn a_rebuild_follows_the_quality_toggle_it_is_given() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    let target = Target {
        format: crate::renderer::post::HDR_FORMAT,
        samples: 1,
    };
    let mut on = QualityLevel::Automatic.profile();
    on.beams = true;
    let mut off = on;
    off.beams = false;
    let beams = [beam()];

    let images = Answered::default();

    let mut pass = Beams::new(&device, &queue, target, &beams, &images, &off);
    assert!(!pass.enabled);
    assert!(pass.live.is_empty());

    pass.rebuild(&device, &queue, &beams, &images, &on);
    assert!(pass.enabled);
    assert_eq!(pass.live.len(), 1);

    pass.rebuild(&device, &queue, &beams, &images, &off);
    assert!(!pass.enabled);
    assert!(pass.live.is_empty());
}
