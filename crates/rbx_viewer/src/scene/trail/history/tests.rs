use glam::Vec3;

use super::*;

#[test]
fn the_first_record_always_starts_a_segment() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 5.0);
    assert_eq!(recorder.samples().len(), 1);
}

#[test]
fn movement_below_min_length_updates_the_last_sample_in_place() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 1.0);
    recorder.record(0.1, Vec3::new(0.5, 0.0, 0.0), Vec3::X, 1.0);

    assert_eq!(recorder.samples().len(), 1);
    let only = recorder.samples().front().unwrap();
    // The endpoint moved to the new position rather than a segment being added.
    assert_eq!(only.position0, Vec3::new(0.5, 0.0, 0.0));
    assert_eq!(
        only.time, 0.0,
        "an in-place nudge keeps the original timestamp"
    );
}

#[test]
fn movement_past_min_length_on_either_attachment_starts_a_new_segment() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 1.0);
    recorder.record(0.1, Vec3::new(2.0, 0.0, 0.0), Vec3::X, 1.0);
    assert_eq!(recorder.samples().len(), 2);

    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 1.0);
    recorder.record(0.1, Vec3::ZERO, Vec3::new(1.0, 2.0, 0.0), 1.0);
    assert_eq!(
        recorder.samples().len(),
        2,
        "Attachment1 moving is enough on its own"
    );
}

#[test]
fn perfectly_stationary_attachments_never_grow_the_history_even_at_zero_min_length() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
    for frame in 1..100 {
        recorder.record(frame as f32 * 0.016, Vec3::ZERO, Vec3::X, 0.0);
    }
    // This is the exact situation this viewer hits every frame today (see
    // `scene::trail`'s module doc): nothing ever moves, so the history must
    // stay a single point, never a segment.
    assert_eq!(recorder.samples().len(), 1);
}

#[test]
fn expire_drops_only_samples_older_than_lifetime() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
    recorder.record(1.0, Vec3::new(5.0, 0.0, 0.0), Vec3::X, 1.0);
    recorder.record(2.0, Vec3::new(10.0, 0.0, 0.0), Vec3::X, 1.0);

    recorder.expire(2.5, 2.0);

    let times: Vec<f32> = recorder.samples().iter().map(|s| s.time).collect();
    assert_eq!(
        times,
        vec![1.0, 2.0],
        "only the t=0 sample is older than Lifetime=2 at now=2.5"
    );
}

#[test]
fn expire_can_clear_the_whole_history() {
    let mut recorder = Recorder::new();
    recorder.record(0.0, Vec3::ZERO, Vec3::X, 0.0);
    recorder.record(1.0, Vec3::new(5.0, 0.0, 0.0), Vec3::X, 1.0);

    recorder.expire(10.0, 2.0);
    assert!(recorder.samples().is_empty());
}
