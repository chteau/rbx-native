use glam::Vec3;

use super::*;
use crate::clip::{Marker, PoseKey};

fn hold(joint: &str, x: f32, priority: Priority) -> Clip {
    let mut clip = Clip {
        priority,
        ..Clip::default()
    };
    clip.set_pose(0., joint, PoseKey::new(Frame::at(Vec3::new(x, 0., 0.))));
    clip.set_pose(1., joint, PoseKey::new(Frame::at(Vec3::new(x, 0., 0.))));
    clip
}

fn x(animator: &Animator, joint: &str) -> f32 {
    animator.pose()[joint].position.x
}

#[test]
fn fades_in_linearly_and_partial_weight_mixes_with_rest() {
    let mut a = Animator::new();
    let t = a.load(hold("Head", 4., Priority::Action));
    a.play(t, 0.5, 1., 1.);
    a.step(0.25);
    assert!((a.track(t).unwrap().weight() - 0.5).abs() < 1e-5);
    assert!((x(&a, "Head") - 2.).abs() < 1e-4);
    a.step(0.5);
    assert!((x(&a, "Head") - 4.).abs() < 1e-4);
}

#[test]
fn higher_priority_overrides_and_core_is_lowest() {
    let mut a = Animator::new();
    let core = a.load(hold("Head", 1., Priority::Core));
    let idle = a.load(hold("Head", 2., Priority::Idle));
    a.play(core, 0., 1., 1.);
    a.play(idle, 0., 1., 1.);
    a.step(0.1);
    assert!((x(&a, "Head") - 2.).abs() < 1e-4, "Idle outranks Core");

    a.adjust_weight(idle, 0.25, 0.);
    a.step(0.);
    // 0.25 of Idle, then Core fills the remaining 0.75 (its 1 is normalised
    // against the 1.25 total).
    let blended = x(&a, "Head");
    assert!(
        (blended - (0.25 * 2. + 1. * 1.) / 1.25).abs() < 1e-4,
        "{blended}"
    );
}

#[test]
fn equal_priorities_blend_by_weight() {
    let mut a = Animator::new();
    let one = a.load(hold("Head", 0., Priority::Action));
    let two = a.load(hold("Head", 3., Priority::Action));
    a.play(one, 0., 1., 1.);
    a.play(two, 0., 2., 1.);
    a.step(0.);
    assert!((x(&a, "Head") - 2.).abs() < 1e-4);
}

#[test]
fn stop_fades_out_then_reports_stopped() {
    let mut a = Animator::new();
    let t = a.load(hold("Head", 1., Priority::Action));
    a.play(t, 0., 1., 1.);
    a.stop(t, 0.2);
    assert!(a.step(0.1).is_empty());
    assert_eq!(a.step(0.1), [Event::Stopped(t)]);
    assert_eq!(a.playing().count(), 0);
    assert!(a.pose().is_empty());
}

#[test]
fn markers_fire_once_and_loops_report() {
    let mut clip = hold("Head", 0., Priority::Action);
    clip.looped = true;
    let k = clip.keyframe_mut(0.5);
    k.name = "Mid".into();
    k.markers.push(Marker {
        name: "Step".into(),
        value: "L".into(),
    });
    let mut a = Animator::new();
    let t = a.load(clip);
    a.play(t, 0., 1., 1.);
    let first = a.step(0.6);
    assert_eq!(first.len(), 2);
    assert!(
        matches!(&first[1], Event::MarkerReached { name, value, .. } if name == "Step" && value == "L")
    );
    let wrapped = a.step(1.0);
    assert!(wrapped.contains(&Event::DidLoop(t)));
    assert!((a.track(t).unwrap().time - 0.6).abs() < 1e-5);
}

#[test]
fn a_clip_that_does_not_loop_ends() {
    let mut a = Animator::new();
    let t = a.load(hold("Head", 1., Priority::Action));
    a.play(t, 0., 1., 2.);
    let events = a.step(0.75);
    assert_eq!(events, [Event::Ended(t), Event::Stopped(t)]);
    assert!(!a.track(t).unwrap().playing);
}

#[test]
fn average_of_nothing_is_none() {
    assert!(average(&[]).is_none());
    assert!(average(&[(Frame::IDENTITY, 0.)]).is_none());
}
