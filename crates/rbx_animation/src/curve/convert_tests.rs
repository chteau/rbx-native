use std::f32::consts::PI;

use glam::{Quat, Vec3};

use super::convert::STEP;
use super::*;
use crate::clip::{Clip, Marker, PoseKey, Priority};
use crate::frame::Frame;
use crate::sample::sample;

const TIMES: [f32; 5] = [0., 0.4, 1.0, 1.7, 2.5];

/// Three joints, each turning about one axis (where Euler channels and the
/// pose slerp take the same path), through angles that cross ±π and pass
/// the middle axis's ±π/2.
fn clip_with(styles: &[(Style, Direction)]) -> Clip {
    let mut clip = Clip {
        name: "Wave".into(),
        looped: false,
        priority: Priority::Movement,
        keyframes: Vec::new(),
    };
    let angles = [0., 1.0, 3.0, -3.0, 0.5];
    let middle = [0., 2.0, -0.5, -2.2, 1.0];
    for (i, &t) in TIMES.iter().enumerate() {
        let (style, direction) = styles[i % styles.len()];
        let pos = Vec3::new(i as f32, (i as f32 * 1.3).sin() * 2., -(i as f32) * 0.5);
        let poses = [
            ("Head", Frame::new(pos, Quat::from_rotation_x(angles[i]))),
            ("Arm", Frame::new(-pos, Quat::from_rotation_y(middle[i]))),
            (
                "Leg",
                Frame::new(pos * 2., Quat::from_rotation_z(-angles[i])),
            ),
        ];
        for (joint, frame) in poses {
            let mut key = PoseKey::new(frame);
            key.style = style;
            key.direction = direction;
            clip.set_pose(t, joint, key);
        }
    }
    clip
}

/// The largest position and rotation gap between the two forms at `time`.
fn gap(clip: &Clip, curves: &CurveClip, time: f32) -> (f32, f32) {
    let want = sample(clip, time);
    let got = curves.sample(time);
    assert_eq!(want.len(), got.len());
    want.iter().fold((0f32, 0f32), |(p, r), (joint, s)| {
        let frame = got[joint];
        (
            p.max(frame.position.distance(s.transform.position)),
            r.max(frame.rotation.angle_between(s.transform.rotation)),
        )
    })
}

fn assert_matches(clip: &Clip, curves: &CurveClip, tolerance: f32, skip: impl Fn(f32) -> bool) {
    for i in 0..=500 {
        let t = clip.length() * i as f32 / 500.;
        if skip(t) {
            continue;
        }
        let (p, r) = gap(clip, curves, t);
        assert!(
            p < tolerance && r < tolerance,
            "at {t}: position {p}, rotation {r}"
        );
    }
}

#[test]
fn linear_clip_samples_the_same() {
    let clip = clip_with(&[(Style::Linear, Direction::In)]);
    let curves = from_clip(&clip, 30.);
    assert_eq!(curves.name, "Wave");
    assert_eq!(curves.priority, Priority::Movement);
    assert_eq!(curves.joints["Head"].rotation[0].keys.len(), TIMES.len());
    assert_matches(&clip, &curves, 1e-3, |_| false);
}

#[test]
fn cubic_clip_samples_the_same() {
    let clip = clip_with(&[
        (Style::CubicV2, Direction::In),
        (Style::CubicV2, Direction::Out),
        (Style::CubicV2, Direction::InOut),
        (Style::Cubic, Direction::In),
    ]);
    let curves = from_clip(&clip, 30.);
    // InOut's segment gains a midpoint key.
    assert_eq!(curves.joints["Arm"].position[1].keys.len(), TIMES.len() + 1);
    assert_matches(&clip, &curves, 1e-3, |_| false);
}

#[test]
fn constant_clip_samples_the_same() {
    let clip = clip_with(&[
        (Style::Constant, Direction::Out),
        (Style::Constant, Direction::InOut),
        (Style::Constant, Direction::In),
    ]);
    let curves = from_clip(&clip, 30.);
    // `In` lags the pose by the step key's offset, and the midpoint swap is
    // a discontinuity either form may land on.
    let near_step = |t: f32| {
        TIMES
            .windows(2)
            .any(|w| (t >= w[0] && t <= w[0] + 2. * STEP) || (t - (w[0] + w[1]) / 2.).abs() < 1e-3)
    };
    assert_matches(&clip, &curves, 1e-3, near_step);
}

#[test]
fn euler_keys_are_unwrapped() {
    let curves = from_clip(&clip_with(&[(Style::Linear, Direction::In)]), 30.);
    for joint in curves.joints.values() {
        for curve in &joint.rotation {
            for pair in curve.keys.windows(2) {
                assert!((pair[1].value - pair[0].value).abs() <= PI, "{pair:?}");
            }
        }
    }
    // 3 → -3 goes the short way, through π.
    let x = &curves.joints["Head"].rotation[0];
    assert!((x.keys[3].value - (2. * PI - 3.)).abs() < 1e-4);
    // The middle axis past π/2 stays on the Y channel instead of flipping.
    let arm = &curves.joints["Arm"];
    assert!((arm.rotation[1].keys[1].value - 2.0).abs() < 1e-4);
    assert!(arm.rotation[0].keys.iter().all(|k| k.value.abs() < 1e-4));
}

#[test]
fn every_order_converts_a_tumbling_joint() {
    let mut clip = Clip::default();
    for (i, &t) in TIMES.iter().enumerate() {
        let f = i as f32;
        let rotation = Quat::from_euler(glam::EulerRot::ZXY, 0.9 * f, -0.6 * f, 1.3 * f);
        clip.set_pose(t, "Head", PoseKey::new(Frame::new(Vec3::ZERO, rotation)));
    }
    for order in RotationOrder::ALL {
        let curves = from_clip_in_order(&clip, 30., order);
        assert_eq!(curves.joints["Head"].order, order);
        // Keys land on the poses exactly, whatever path is taken between.
        for &t in &TIMES {
            let (_, r) = gap(&clip, &curves, t);
            assert!(r < 1e-3, "{order:?} at {t}: {r}");
        }
    }
}

#[test]
fn bounce_and_elastic_are_baked() {
    let bounce = clip_with(&[(Style::Bounce, Direction::Out)]);
    let curves = from_clip(&bounce, 30.);
    let x = &curves.joints["Head"].position[0];
    assert!(x.keys.len() > 60);
    assert!(x
        .keys
        .iter()
        .all(|k| k.interpolation == Interpolation::Linear));
    for k in &x.keys {
        let (p, r) = gap(&bounce, &curves, k.time);
        assert!(p < 1e-4 && r < 1e-3, "at {}: {p} {r}", k.time);
    }
    // Between baked keys only the chord's error remains. Bounce's rebounds
    // are corners, where a chord cuts off up to a quarter of the slope jump
    // times the step: about 0.2 studs on this clip's 4.4-stud span at 120 fps.
    assert_matches(&bounce, &from_clip(&bounce, 120.), 0.25, |_| false);

    let elastic = clip_with(&[(Style::Elastic, Direction::InOut)]);
    let curves = from_clip(&elastic, 30.);
    for k in &curves.joints["Leg"].rotation[2].keys {
        let (p, r) = gap(&elastic, &curves, k.time);
        assert!(p < 1e-3 && r < 1e-3, "at {}: {p} {r}", k.time);
    }
}

#[test]
fn linear_round_trip_keeps_the_keyframes() {
    let clip = clip_with(&[(Style::Linear, Direction::In)]);
    let back = to_clip(&from_clip(&clip, 30.), 30.);
    assert_eq!(back.keyframes.len(), clip.keyframes.len());
    assert_eq!(
        (back.name.as_str(), back.priority),
        ("Wave", Priority::Movement)
    );
    for (a, b) in clip.keyframes.iter().zip(&back.keyframes) {
        assert!((a.time - b.time).abs() < 1e-5);
        for (joint, pose) in &a.poses {
            assert!(
                b.poses[joint].cframe.approx_eq(&pose.cframe, 1e-4),
                "{joint}"
            );
            assert_eq!(b.poses[joint].style, Style::Linear);
        }
    }
}

#[test]
fn cubic_and_constant_round_trips_play_the_same() {
    let cubic = clip_with(&[
        (Style::CubicV2, Direction::InOut),
        (Style::CubicV2, Direction::Out),
    ]);
    let back = to_clip(&from_clip(&cubic, 30.), 120.);
    assert!(back.keyframes.len() > 60);
    let curves = from_clip(&back, 120.);
    assert_matches(&cubic, &curves, 5e-3, |_| false);

    let constant = clip_with(&[(Style::Constant, Direction::Out)]);
    let back = to_clip(&from_clip(&constant, 30.), 30.);
    assert_eq!(back.keyframes.len(), constant.keyframes.len());
    assert!(back.keyframes[0]
        .poses
        .values()
        .all(|p| p.style == Style::Constant));
    for i in 0..=100 {
        let t = 2.5 * i as f32 / 100.;
        let (want, got) = (sample(&constant, t), sample(&back, t));
        for (joint, s) in &want {
            assert!(
                got[joint].transform.approx_eq(&s.transform, 1e-4),
                "{joint} at {t}"
            );
        }
    }
}

#[test]
fn multi_axis_euler_segments_are_baked_on_the_way_back() {
    let mut joint = JointCurves::default();
    for axis in 0..2 {
        joint.rotation[axis] = FloatCurve::new(vec![
            FloatKey::new(0., 0., Interpolation::Linear),
            FloatKey::new(1., 1.5, Interpolation::Linear),
        ]);
    }
    let mut curves = CurveClip::default();
    curves.joints.insert("Head".into(), joint);
    let back = to_clip(&curves, 30.);
    assert_eq!(back.keyframes.len(), 31);
    for i in 0..=90 {
        let t = i as f32 / 90.;
        let want = curves.sample(t)["Head"];
        let got = sample(&back, t)["Head"].transform;
        assert!(want.rotation.angle_between(got.rotation) < 2e-3, "at {t}");
    }
}

#[test]
fn markers_survive_both_ways() {
    let mut clip = clip_with(&[(Style::Linear, Direction::In)]);
    let step = Marker {
        name: "Step".into(),
        value: "Left".into(),
    };
    let end = Marker {
        name: "End".into(),
        value: String::new(),
    };
    clip.keyframe_mut(1.0).markers.push(step.clone());
    clip.keyframe_mut(3.0).markers.push(end.clone());
    let curves = from_clip(&clip, 30.);
    assert_eq!(
        curves.markers,
        vec![(1.0, step.clone()), (3.0, end.clone())]
    );
    assert_eq!(curves.length(), 3.0);

    let back = to_clip(&curves, 30.);
    assert_eq!(back.length(), 3.0);
    assert_eq!(
        back.keyframes[back.keyframe_index(1.0).unwrap()].markers,
        vec![step]
    );
    let last = back.keyframes.last().unwrap();
    assert!(last.poses.is_empty());
    assert_eq!(last.markers, vec![end]);
}
