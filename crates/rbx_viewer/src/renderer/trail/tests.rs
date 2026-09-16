use glam::Vec3;
use rbx_dom::{ColorSequence, NumberSequence, Ref};

use super::*;
use crate::quality::QualityLevel;

/// A textureless trail: `AssetRef::Empty` draws through the built-in white
/// slot, so building it fetches nothing.
fn trail() -> Trail {
    Trail {
        position0: Vec3::ZERO,
        position1: Vec3::X,
        enabled: true,
        lifetime: 2.0,
        min_length: 0.0,
        width_scale: NumberSequence {
            keypoints: Vec::new(),
        },
        color: ColorSequence {
            keypoints: Vec::new(),
        },
        transparency: NumberSequence {
            keypoints: Vec::new(),
        },
        texture: AssetRef::Empty,
        texture_length: 1.0,
        light_emission: 0.0,
        referent: Ref::new(1),
    }
}

// A quality level switched between two scenes has to take on the next
// rebuild: whether trails draw is read from the profile every rebuild, not
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
    on.trails = true;
    let mut off = on;
    off.trails = false;
    let trails = [trail()];

    let mut pass = Trails::new(&device, &queue, target, &trails, &off);
    assert!(!pass.enabled);
    assert!(pass.live.is_empty());

    pass.rebuild(&device, &queue, &trails, &on);
    assert!(pass.enabled);
    assert_eq!(pass.live.len(), 1);

    pass.rebuild(&device, &queue, &trails, &off);
    assert!(!pass.enabled);
    assert!(pass.live.is_empty());
}
