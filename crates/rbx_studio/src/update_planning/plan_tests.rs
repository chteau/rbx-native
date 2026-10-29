use super::*;

const H: i64 = 3_600;
const T: i64 = 1_000_000;

fn plan(now: i64) -> Plan {
    let timers = default_timers();
    let images: Vec<String> = (0..timers.len()).map(|i| format!("step-{i}.png")).collect();
    Plan {
        universe_id: 1,
        experience: "Pebble Kart".into(),
        language: "en".into(),
        update_at: T,
        started_at: now,
        steps: build_steps(&timers, &images, "new.png", T, now),
        cancelled: false,
    }
}

fn labels(plan: &Plan) -> Vec<&str> {
    plan.steps.iter().map(|s| s.label.as_str()).collect()
}

#[test]
fn steps_run_from_the_longest_timer_to_the_new_icon() {
    let plan = plan(T - 24 * H);
    assert_eq!(
        labels(&plan),
        ["12 Hours", "6 Hours", "3 Hours", "1 Hour", "New icon"]
    );
    let ats: Vec<i64> = plan.steps.iter().map(|s| s.at).collect();
    assert_eq!(ats, [T - 12 * H, T - 6 * H, T - 3 * H, T - H, T]);
    assert_eq!(plan.steps[4].image, "new.png");
}

#[test]
fn a_late_start_keeps_only_the_timer_due_now() {
    let plan = plan(T - 4 * H);
    assert_eq!(labels(&plan), ["6 Hours", "3 Hours", "1 Hour", "New icon"]);
    assert_eq!(plan.to_apply(T - 4 * H), Some(0));
}

#[test]
fn nothing_is_due_before_the_first_timer() {
    let plan = plan(T - 24 * H);
    assert_eq!(plan.to_apply(T - 13 * H), None);
    assert_eq!(plan.next_at(T - 13 * H), Some(T - 12 * H));
    assert_eq!(plan.state(0, T - 13 * H), StepState::Upcoming);
}

#[test]
fn each_step_is_applied_once() {
    let mut plan = plan(T - 24 * H);
    let now = T - 12 * H;
    assert_eq!(plan.to_apply(now), Some(0));
    plan.mark_applied(0, now);
    assert_eq!(plan.to_apply(now + 60), None);
    assert_eq!(plan.state(0, now + 60), StepState::Applied);
    assert_eq!(plan.state(1, now + 60), StepState::Upcoming);
}

#[test]
fn waking_after_missed_steps_applies_the_latest_and_skips_the_rest() {
    let mut plan = plan(T - 24 * H);
    plan.mark_applied(0, T - 12 * H);
    // Off from H-7 to H-0:30: H-6, H-3 and H-1 all passed.
    let wake = T - H / 2;
    assert_eq!(plan.state(1, wake), StepState::Skipped);
    assert_eq!(plan.to_apply(wake), Some(3));
    plan.mark_applied(3, wake);
    assert!(plan.steps[1].skipped && plan.steps[2].skipped);
    assert!(!plan.steps[0].skipped);
    assert_eq!(plan.next_at(wake), Some(T));
    assert!(!plan.finished());
}

#[test]
fn waking_after_the_update_applies_the_new_icon() {
    let mut plan = plan(T - 24 * H);
    assert_eq!(plan.to_apply(T + 5 * H), Some(4));
    plan.mark_applied(4, T + 5 * H);
    assert!(plan.finished());
    assert_eq!(plan.next_at(T + 5 * H), None);
}

#[test]
fn cancel_stops_every_later_step() {
    let mut plan = plan(T - 24 * H);
    plan.mark_applied(0, T - 12 * H);
    plan.cancel();
    assert!(plan.finished());
    assert_eq!(plan.to_apply(T - 5 * H), None);
    assert_eq!(plan.next_at(T - 5 * H), None);
    assert_eq!(plan.state(0, T - 5 * H), StepState::Applied);
    assert_eq!(plan.state(3, T - 5 * H), StepState::Cancelled);
}

#[test]
fn a_failed_apply_stays_due_and_is_retried() {
    let mut plan = plan(T - 24 * H);
    plan.mark_failed(0, "HTTP 503".into());
    assert_eq!(plan.state(0, T - 12 * H), StepState::Due);
    assert_eq!(plan.to_apply(T - 12 * H + 60), Some(0));
    plan.mark_applied(0, T - 12 * H + 60);
    assert_eq!(plan.steps[0].error, None);
}

#[test]
fn timers_at_the_update_or_twice_at_one_time_collapse() {
    let mut timers = default_timers();
    timers.push(Timer {
        label: "Now".into(),
        minutes_before: 0,
        overlay: timers[0].overlay.clone(),
    });
    timers.push(Timer {
        label: "Also 6".into(),
        minutes_before: 360,
        overlay: timers[0].overlay.clone(),
    });
    let images: Vec<String> = (0..timers.len()).map(|i| i.to_string()).collect();
    let steps = build_steps(&timers, &images, "n", T, 0);
    let labels: Vec<_> = steps.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        ["12 Hours", "6 Hours", "3 Hours", "1 Hour", "New icon"]
    );
}

#[test]
fn review_waits_only_on_uploaded_steps() {
    let mut plan = plan(T - 24 * H);
    plan.steps[0].asset_id = Some(11);
    plan.steps[0].review = Review::Uploaded;
    plan.steps[1].asset_id = Some(12);
    plan.steps[1].review = Review::Approved;
    assert_eq!(plan.awaiting_review(), [(0, 11)]);
    assert_eq!(Review::from(Moderation::Reviewing), Review::Uploaded);
    assert_eq!(Review::from(Moderation::Approved), Review::Approved);
}

#[test]
fn countdown_reads_at_every_scale() {
    assert_eq!(countdown(2 * 86_400 + 4 * H + 5), "2d 4h");
    assert_eq!(countdown(3 * H + 5 * 60), "3h 05m");
    assert_eq!(countdown(4 * 60 + 9), "4m 09s");
    assert_eq!(countdown(12), "12s");
    assert_eq!(countdown(-3), "0s");
}
