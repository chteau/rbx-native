use std::time::Duration;

use std::time::Instant;

use super::{deadline, due_pose, step};

const INTERVAL: Duration = Duration::from_millis(13);

#[test]
fn a_frame_advances_the_camera_by_its_own_length() {
    assert_eq!(step(Duration::from_millis(13), INTERVAL), INTERVAL);
    assert_eq!(
        step(Duration::from_millis(7), INTERVAL),
        Duration::from_millis(7)
    );
}

// Waking from a minute of idling must not move the camera by a minute of
// flight, which at 50 studs a second lands it outside the map.
#[test]
fn a_long_gap_is_capped_rather_than_flown_through() {
    assert_eq!(step(Duration::from_secs(60), INTERVAL), INTERVAL * 3);
    assert_eq!(step(Duration::from_millis(40), INTERVAL), INTERVAL * 3);
}

#[test]
fn each_frame_is_due_one_budget_after_the_last() {
    let first = Instant::now();
    let second = deadline(first, INTERVAL, first);
    assert_eq!(second, first + INTERVAL);
    // Two milliseconds late: the next frame is still due on the original
    // beat, which is what makes the overshoot up.
    assert_eq!(
        deadline(second, INTERVAL, second + Duration::from_millis(2)),
        second + INTERVAL
    );
}

// Catching up on a whole second of missed frames would run the loop flat out
// with no sleep at all until it had.
#[test]
fn a_frame_that_ran_long_gives_up_on_the_beat_it_missed() {
    let started = Instant::now();
    let late = started + Duration::from_secs(1);
    assert_eq!(deadline(started, INTERVAL, late), late);
}

// Still orbiting: `Headless::pose` reports `None`, so there is nothing to sync.
#[test]
fn no_pose_yet_reports_nothing() {
    assert_eq!(due_pose(None::<u8>, None, true), None);
}

// The throttle gate: due, but the camera has not actually moved since the
// last report, so re-sending it would only spam the DOM write with a no-op.
#[test]
fn an_unchanged_pose_is_not_resent_even_when_due() {
    assert_eq!(due_pose(Some(1), Some(1), true), None);
}

// A moved pose still waits for `POSE_SYNC_INTERVAL` — this is what keeps the
// DOM write to a few times a second instead of once a rendered frame.
#[test]
fn a_moved_pose_is_held_back_until_the_interval_is_due() {
    assert_eq!(due_pose(Some(2), Some(1), false), None);
}

#[test]
fn a_moved_pose_reports_once_due() {
    assert_eq!(due_pose(Some(2), Some(1), true), Some(2));
}

// The very first pose, before anything has ever been sent, is due immediately
// — see `run`'s `pose_due` initialization: a place opened at its own saved
// `Camera` still reports that pose once, with zero input.
#[test]
fn the_first_pose_is_reported_with_nothing_sent_before_it() {
    assert_eq!(due_pose(Some(1), None, true), Some(1));
}
