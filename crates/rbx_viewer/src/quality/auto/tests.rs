use std::time::Duration;

use super::*;

const TARGET_HZ: f32 = 60.0;

fn budget() -> Duration {
    Duration::from_secs_f32(1.0 / TARGET_HZ)
}

/// A frame costing `share` of the budget: 1.0 is exactly on target, 1.5 is a
/// machine delivering 40 fps where 60 was asked for.
fn frame(share: f32) -> Duration {
    budget().mul_f32(share)
}

/// Feeds frames of a fixed cost until `span` of them has been recorded, and
/// reports every level the manager moved to along the way.
fn feed(manager: &mut FrameRateManager, span: Duration, share: f32) -> Vec<u8> {
    let mut trail = Vec::new();
    let mut spent = Duration::ZERO;
    while spent < span {
        manager.record(frame(share));
        spent += frame(share);
        if let Some(level) = manager.changed() {
            trail.push(level);
        }
    }

    trail
}

/// Feeds frames whose cost depends on the level they were drawn at — a machine
/// that gets faster as the level comes down, which is the point of the manager.
fn drive(manager: &mut FrameRateManager, span: Duration, cost: impl Fn(u8) -> f32) -> Vec<u8> {
    let mut trail = Vec::new();
    let mut spent = Duration::ZERO;
    while spent < span {
        let took = frame(cost(manager.level()));
        manager.record(took);
        spent += took;
        if let Some(level) = manager.changed() {
            trail.push(level);
        }
    }

    trail
}

#[test]
fn a_machine_well_inside_its_budget_stays_at_the_top_level() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    assert_eq!(manager.level(), QualityLevel::MAX);

    let trail = feed(&mut manager, Duration::from_secs(30), 0.2);
    assert!(trail.is_empty(), "{trail:?}");
    assert_eq!(manager.level(), QualityLevel::MAX);
}

// The first frames are the estimate the start level rests on: nothing may move
// before there are enough of them to average.
#[test]
fn nothing_moves_before_the_warm_up_is_over() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    for _ in 0..WARMUP_FRAMES {
        manager.record(frame(4.0));
        assert_eq!(manager.changed(), None);
    }
    assert_eq!(manager.level(), QualityLevel::MAX);

    let trail = feed(&mut manager, Duration::from_secs(1), 4.0);
    assert_eq!(trail.first(), Some(&20));
}

#[test]
fn a_machine_over_budget_walks_down_one_level_at_a_time() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    let trail = feed(&mut manager, Duration::from_secs(4), 1.5);

    assert_eq!(trail.first(), Some(&20));
    for pair in trail.windows(2) {
        assert_eq!(pair[1] + 1, pair[0], "{trail:?}");
    }
}

// Nothing ever asks the renderer for a level it has no profile for.
#[test]
fn a_machine_that_never_catches_up_stops_at_the_bottom_level() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    let trail = feed(&mut manager, Duration::from_secs(60), 3.0);

    assert_eq!(trail.last(), Some(&QualityLevel::MIN));
    assert_eq!(manager.level(), QualityLevel::MIN);
    assert!(trail.iter().all(|level| *level >= QualityLevel::MIN));
}

#[test]
fn a_machine_settles_at_the_level_it_can_hold_and_stays_there() {
    // Everything above 12 costs it 40 % more than it has; 12 and below leaves it
    // 15 % of the budget spare — not enough headroom to climb back.
    let cost = |level: u8| if level > 12 { 1.4 } else { 0.85 };

    let mut manager = FrameRateManager::new(TARGET_HZ);
    let walked = drive(&mut manager, Duration::from_secs(20), cost);
    assert_eq!(walked, (12..=20).rev().collect::<Vec<u8>>());

    let settled = drive(&mut manager, Duration::from_secs(120), cost);
    assert!(settled.is_empty(), "{settled:?}");
    assert_eq!(manager.level(), 12);
}

// A machine whose frames alternate between comfortable and late averages out on
// budget, and a level that flickers reads as a bug rather than as a compromise.
#[test]
fn an_alternating_machine_does_not_flap() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    let mut late = false;
    for _ in 0..4_000 {
        late = !late;
        manager.record(frame(if late { 1.4 } else { 0.5 }));
        assert_eq!(manager.changed(), None);
    }

    assert_eq!(manager.level(), QualityLevel::MAX);
}

#[test]
fn a_machine_far_under_budget_climbs_four_levels_at_a_time() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    drive(&mut manager, Duration::from_secs(20), |level| {
        if level > 12 {
            1.4
        } else {
            0.85
        }
    });
    assert_eq!(manager.level(), 12);

    let climbed = feed(&mut manager, Duration::from_secs(60), 0.1);
    assert_eq!(climbed, vec![16, 20, QualityLevel::MAX]);
}

// A climb that starts the moment the frames come good would undo the step that
// made them good, which is how a level ends up oscillating for ever.
#[test]
fn a_step_down_holds_a_climb_back_for_five_seconds() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    assert_eq!(
        feed(&mut manager, Duration::from_secs(3), 1.5).first(),
        Some(&20)
    );
    let dropped = manager.level();

    let held = feed(&mut manager, Duration::from_millis(4_500), 0.1);
    assert!(held.is_empty(), "{held:?}");
    assert_eq!(manager.level(), dropped);

    let released = feed(&mut manager, Duration::from_millis(1_500), 0.1);
    assert!(!released.is_empty(), "the hysteresis never expired");
}

// A single frame reported as taking longer than the whole window is a stall, not
// a sample to be averaged away by the frames around it.
#[test]
fn one_enormous_frame_is_answered_rather_than_smoothed_out() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    feed(&mut manager, Duration::from_secs(1), 0.2);

    manager.record(Duration::from_secs(2));
    assert_eq!(manager.changed(), Some(20));
}

#[test]
fn the_level_is_only_ever_handed_over_once() {
    let mut manager = FrameRateManager::new(TARGET_HZ);
    let trail = feed(&mut manager, Duration::from_secs(2), 2.0);

    assert_eq!(trail.first(), Some(&20));
    assert_eq!(manager.level(), *trail.last().expect("a step down"));
    // Already collected by `feed`: a decision handed over twice is a second
    // switch for nothing.
    assert_eq!(manager.changed(), None);
}

// A caller with no display to ask reports whatever it has; a budget of zero or
// of infinity would divide the arithmetic by one or the other.
#[test]
fn an_impossible_target_still_yields_a_usable_budget() {
    for target in [0.0, -60.0, f32::INFINITY, f32::NAN] {
        let mut manager = FrameRateManager::new(target);
        feed(&mut manager, Duration::from_secs(5), 1.0);
        let level = manager.level();
        assert!(
            (QualityLevel::MIN..=QualityLevel::MAX).contains(&level),
            "{target} gave level {level}"
        );
    }
}
