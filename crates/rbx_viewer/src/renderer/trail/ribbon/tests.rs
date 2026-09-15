use rbx_assets::AssetRef;
use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref,
};

use super::*;
use crate::scene::Trail;

const EPSILON: f32 = 1e-4;

fn trail() -> Trail {
    Trail {
        position0: Vec3::ZERO,
        position1: Vec3::new(2.0, 0.0, 0.0),
        enabled: true,
        lifetime: 2.0,
        min_length: 0.0,
        // Wide at the newest end (t=0), narrow at the oldest (t=1): the
        // vertex-count/width-at-both-ends assertions below only prove
        // anything if the two ends are distinguishable.
        width_scale: NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: 1.0,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: 0.25,
                    envelope: 0.0,
                },
            ],
        },
        color: ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
                envelope: 0.0,
            }],
        },
        transparency: NumberSequence {
            keypoints: vec![NumberSequenceKeypoint {
                time: 0.0,
                value: 0.0,
                envelope: 0.0,
            }],
        },
        texture: AssetRef::Empty,
        texture_length: 1.0,
        light_emission: 0.0,
        referent: Ref::new(1),
    }
}

/// Replays a scripted attachment motion — both attachments moving together
/// along +X — through the exact same `Recorder`/`trail_segments` code the
/// live renderer calls every frame, the "synthetic test/debug path" this
/// viewer needs since nothing here ever moves an `Attachment` for real (see
/// `scene::trail`'s module doc).
fn scripted_motion(samples: &[(f32, f32)], min_length: f32) -> TrailRecorder {
    let mut recorder = TrailRecorder::new();
    for &(time, x) in samples {
        let position0 = Vec3::new(x, 0.0, 0.0);
        let position1 = Vec3::new(x + 2.0, 0.0, 0.0);
        recorder.record(time, position0, position1, min_length);
    }
    recorder
}

#[test]
fn no_recorded_history_draws_nothing() {
    let recorder = TrailRecorder::new();
    assert!(vertices(&trail(), &recorder, Vec3::new(0.0, 0.0, 10.0), 0.0).is_empty());
}

#[test]
fn a_single_sample_draws_nothing() {
    let mut recorder = TrailRecorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
    assert!(vertices(&trail(), &recorder, Vec3::new(0.0, 0.0, 10.0), 0.0).is_empty());
}

#[test]
fn a_scripted_motion_path_yields_two_vertices_per_recorded_sample() {
    let recorder = scripted_motion(&[(0.0, 0.0), (0.5, 5.0), (1.0, 10.0)], 1.0);
    let verts = vertices(&trail(), &recorder, Vec3::new(5.0, 0.0, 10.0), 1.0);
    assert_eq!(verts.len(), 6, "3 samples * 2 edge vertices each");
}

#[test]
fn width_is_widest_at_the_newest_end_and_narrowest_at_the_oldest() {
    let recorder = scripted_motion(&[(0.0, 0.0), (0.5, 5.0), (1.0, 10.0)], 1.0);
    let now = 1.0;
    let eye = Vec3::new(5.0, 0.0, 10.0);
    let verts = vertices(&trail(), &recorder, eye, now);
    assert_eq!(verts.len(), 6);

    // Oldest recorded sample (t=0.0, age=1.0/Lifetime=2.0 -> t=0.5, but the
    // *first* two vertices are the oldest sample in `trail_segments`' oldest-
    // first ordering) vs. the newest (last two vertices, age=0, t=0 -> the
    // full WidthScale=1.0).
    let oldest_width = (Vec3::from(verts[0].position) - Vec3::from(verts[1].position)).length();
    let newest_width = (Vec3::from(verts[4].position) - Vec3::from(verts[5].position)).length();

    // position1 - position0 has length 2 studs throughout this fixture; the
    // newest sample (age 0) reads WidthScale's t=0 keypoint (1.0), so its
    // full edge-to-edge width is 2.0 studs.
    assert!(
        (newest_width - 2.0).abs() < EPSILON,
        "newest width was {newest_width}"
    );
    assert!(
        newest_width > oldest_width,
        "newest ({newest_width}) should be wider than oldest ({oldest_width}) per this fixture's WidthScale"
    );
}

#[test]
fn append_bridges_two_trails_with_degenerate_triangles() {
    let recorder = scripted_motion(&[(0.0, 0.0), (1.0, 10.0)], 1.0);
    let one = vertices(&trail(), &recorder, Vec3::new(5.0, 0.0, 10.0), 1.0);
    let mut combined = one.clone();
    append(&mut combined, &one);
    // Original vertices from both trails, plus two degenerate bridge
    // vertices repeating the seam — identical bookkeeping to
    // `renderer::beam::ribbon::append`.
    assert_eq!(combined.len(), one.len() * 2 + 2);
}
