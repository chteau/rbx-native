use glam::Vec3;
use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint,
};

use super::*;

fn number_sequence(points: &[(f32, f32)]) -> NumberSequence {
    NumberSequence {
        keypoints: points
            .iter()
            .map(|&(time, value)| NumberSequenceKeypoint {
                time,
                value,
                envelope: 0.0,
            })
            .collect(),
    }
}

fn color_sequence(points: &[(f32, [f32; 3])]) -> ColorSequence {
    ColorSequence {
        keypoints: points
            .iter()
            .map(|&(time, [r, g, b])| ColorSequenceKeypoint {
                time,
                color: Color3Data { r, g, b },
                envelope: 0.0,
            })
            .collect(),
    }
}

/// A red-at-birth, blue-at-expiry, thin-at-birth, thick-at-expiry, opaque-to-
/// transparent trail: exercises every sequence with a distinct value at each
/// end so a mix-up is unmistakable.
fn trail(lifetime: f32) -> Trail {
    Trail {
        position0: Vec3::ZERO,
        position1: Vec3::ZERO,
        enabled: true,
        lifetime,
        min_length: 0.0,
        width_scale: number_sequence(&[(0.0, 0.2), (1.0, 0.8)]),
        color: color_sequence(&[(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])]),
        transparency: number_sequence(&[(0.0, 0.0), (1.0, 1.0)]),
        texture: rbx_assets::AssetRef::Empty,
        texture_length: 1.0,
        light_emission: 0.0,
        referent: rbx_dom::Ref::new(1),
    }
}

#[test]
fn fewer_than_two_samples_yields_no_ribbon_points() {
    let recorder = Recorder::new();
    assert!(segments(&trail(2.0), &recorder, 0.0).is_empty());

    let mut one_sample = Recorder::new();
    one_sample.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
    assert!(segments(&trail(2.0), &one_sample, 0.0).is_empty());
}

#[test]
fn the_newest_sample_reads_sequence_time_zero_and_the_oldest_reads_time_one() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 1.0);
    recorder.record(2.0, Vec3::new(5.0, 0.0, 0.0), Vec3::X, 1.0);

    let points = segments(&trail(2.0), &recorder, 2.0);
    assert_eq!(points.len(), 2);

    // Oldest recorded (time=0, age=2, t=1): the trail's about-to-expire end.
    let oldest = &points[0];
    assert_eq!(oldest.color, [0.0, 0.0, 1.0]);
    assert_eq!(oldest.width_scale, 0.8);
    assert_eq!(oldest.alpha, 0.0, "Transparency=1 at t=1 means alpha=0");

    // Newest recorded (time=2, age=0, t=0): still right at the attachments.
    let newest = &points[1];
    assert_eq!(newest.color, [1.0, 0.0, 0.0]);
    assert_eq!(newest.width_scale, 0.2);
    assert_eq!(
        newest.alpha, 1.0,
        "Transparency=0 at t=0 means fully opaque"
    );
}

#[test]
fn age_beyond_lifetime_clamps_to_the_sequences_far_end() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 1.0);
    recorder.record(1.0, Vec3::new(5.0, 0.0, 0.0), Vec3::X, 1.0);

    // `now` far past the first sample's Lifetime window: its age/Lifetime
    // ratio would exceed 1 without clamping.
    let points = segments(&trail(2.0), &recorder, 100.0);
    assert_eq!(points[0].width_scale, 0.8);
    assert_eq!(points[0].color, [0.0, 0.0, 1.0]);
}

#[test]
fn distance_accumulates_along_the_midpoint_path() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::ZERO, 1.0);
    recorder.record(1.0, Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), 1.0);
    recorder.record(2.0, Vec3::new(4.0, 3.0, 0.0), Vec3::new(4.0, 3.0, 0.0), 1.0);

    let points = segments(&trail(10.0), &recorder, 2.0);
    assert_eq!(points[0].distance, 0.0);
    assert!((points[1].distance - 4.0).abs() < 1e-5);
    assert!(
        (points[2].distance - 7.0).abs() < 1e-5,
        "4 + 3 = 7 studs travelled"
    );
}
