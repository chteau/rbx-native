use std::time::Duration;

use std::time::Instant;

use super::{coalesce, deadline, due_pose, step, Command};

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

fn batch(referent: u32) -> Command {
    Command::Changes(
        Vec::new(),
        vec![rbx_dom::Change::Property {
            referent: rbx_dom::Ref::new(referent),
            name: String::from("CFrame"),
        }],
    )
}

// The bug this exists for: a drag over a large place queued one change batch
// per mouse-move event, each paying the full per-batch cost, and the render
// thread fell behind the mouse for the length of the gesture.
#[test]
fn consecutive_change_batches_fold_into_one_in_order() {
    let folded = coalesce(vec![batch(1), batch(2), batch(3)]);

    assert_eq!(folded.len(), 1);
    let Command::Changes(_, changes) = &folded[0] else {
        panic!("expected one change batch");
    };
    let referents: Vec<u32> = changes
        .iter()
        .map(|change| match change {
            rbx_dom::Change::Property { referent, .. } => referent.value(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(referents, [1, 2, 3]);
}

#[test]
fn another_command_between_two_batches_keeps_them_apart_and_in_place() {
    let folded = coalesce(vec![batch(1), Command::Visible(false), batch(2), batch(3)]);

    assert_eq!(folded.len(), 3);
    assert!(matches!(&folded[0], Command::Changes(_, changes) if changes.len() == 1));
    assert!(matches!(folded[1], Command::Visible(false)));
    assert!(matches!(&folded[2], Command::Changes(_, changes) if changes.len() == 2));
}

fn lines(layer: usize, count: usize) -> Command {
    let segment = rbx_viewer::Segment {
        from: glam::Vec3::ZERO,
        to: glam::Vec3::X,
        color: [1.0; 4],
        on_top: false,
        width: 1.0,
    };
    Command::Lines(layer, vec![segment; count])
}

// A drag's guides send a list between every two change batches; the
// batches must still fold, and each layer keep only its latest list.
#[test]
fn line_lists_between_batches_fold_away_to_the_last_one() {
    let folded = coalesce(vec![batch(1), lines(1, 1), batch(2), lines(1, 2)]);

    assert_eq!(folded.len(), 2);
    assert!(matches!(&folded[0], Command::Changes(_, changes) if changes.len() == 2));
    assert!(matches!(&folded[1], Command::Lines(1, segments) if segments.len() == 2));
}

#[test]
fn each_line_layer_keeps_its_own_latest_list() {
    let folded = coalesce(vec![lines(0, 5), batch(1), lines(1, 1), lines(1, 3)]);

    assert_eq!(folded.len(), 3);
    assert!(matches!(&folded[0], Command::Lines(0, segments) if segments.len() == 5));
    assert!(matches!(&folded[1], Command::Changes(..)));
    assert!(matches!(&folded[2], Command::Lines(1, segments) if segments.len() == 3));
}

#[test]
fn nothing_queued_folds_to_nothing() {
    assert!(coalesce(Vec::new()).is_empty());
}
