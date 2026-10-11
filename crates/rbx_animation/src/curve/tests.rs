use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use super::*;
use crate::frame::Frame;

fn key(time: f32, value: f32, mode: Interpolation) -> FloatKey {
    FloatKey::new(time, value, mode)
}

fn cubic(time: f32, value: f32, left: Option<f32>, right: Option<f32>) -> FloatKey {
    FloatKey {
        left_tangent: left,
        right_tangent: right,
        ..key(time, value, Interpolation::Cubic)
    }
}

#[test]
fn interpolation_values_match_the_enum() {
    assert_eq!(Interpolation::Constant.value(), 0);
    assert_eq!(Interpolation::Linear.value(), 1);
    assert_eq!(Interpolation::Cubic.value(), 2);
    for mode in Interpolation::ALL {
        assert_eq!(Interpolation::from_value(mode.value()), mode);
    }
}

#[test]
fn empty_curve_is_none_and_ends_hold() {
    assert_eq!(FloatCurve::default().eval(1.), None);
    let curve = FloatCurve::new(vec![
        key(1., 2., Interpolation::Linear),
        key(3., 6., Interpolation::Linear),
    ]);
    assert_eq!(curve.eval(0.), Some(2.));
    assert_eq!(curve.eval(9.), Some(6.));
    assert_eq!(curve.length(), 3.);
}

#[test]
fn constant_holds_and_linear_lerps() {
    let curve = FloatCurve::new(vec![
        key(0., 0., Interpolation::Constant),
        key(1., 4., Interpolation::Linear),
        key(2., 8., Interpolation::Linear),
    ]);
    assert_eq!(curve.eval(0.99), Some(0.));
    assert_eq!(curve.eval(1.), Some(4.));
    assert!((curve.eval(1.25).unwrap() - 5.).abs() < 1e-6);
}

#[test]
fn hermite_hits_the_keys_exactly() {
    let curve = FloatCurve::new(vec![
        cubic(0., 1., None, Some(5.)),
        cubic(2., -3., Some(-7.), Some(2.)),
        cubic(3., 4., Some(0.5), None),
    ]);
    for k in &curve.keys {
        assert!((curve.eval(k.time).unwrap() - k.value).abs() < 1e-6);
    }
    // Just short of a key the cubic is continuous with it.
    assert!((curve.eval(2. - 1e-4).unwrap() + 3.).abs() < 1e-2);
}

#[test]
fn tangents_are_slopes_per_second() {
    for h in [0.5, 2., 4.] {
        let curve = FloatCurve::new(vec![
            cubic(0., 0., None, Some(3.)),
            cubic(h, 1., Some(-2.), None),
        ]);
        let d = 1e-4;
        let start = (curve.eval(d).unwrap() - curve.eval(0.).unwrap()) / d;
        let end = (curve.eval(h).unwrap() - curve.eval(h - d).unwrap()) / d;
        assert!((start - 3.).abs() < 2e-2, "h {h}: {start}");
        assert!((end + 2.).abs() < 2e-2, "h {h}: {end}");
    }
}

#[test]
fn unset_tangents_mirror_their_opposite_or_read_as_zero() {
    assert_eq!(cubic(0., 0., None, None).tangents(), (0., 0.));
    assert_eq!(cubic(0., 0., Some(2.), None).tangents(), (2., 2.));
    assert_eq!(cubic(0., 0., None, Some(-1.)).tangents(), (-1., -1.));
    assert_eq!(cubic(0., 0., Some(1.), Some(4.)).tangents(), (1., 4.));
    // Two auto keys: zero slopes give smoothstep.
    let curve = FloatCurve::new(vec![cubic(0., 0., None, None), cubic(1., 1., None, None)]);
    assert!((curve.eval(0.25).unwrap() - 0.15625).abs() < 1e-6);
}

#[test]
fn editing_keeps_order_and_replaces_same_time_keys() {
    let mut curve = FloatCurve::default();
    curve.insert(key(2., 2., Interpolation::Linear));
    curve.insert(key(0., 0., Interpolation::Linear));
    assert_eq!(curve.insert(key(1., 1., Interpolation::Linear)), 1);
    assert_eq!(curve.insert(key(1., 9., Interpolation::Linear)), 1);
    assert_eq!(curve.keys.len(), 3);
    assert_eq!(curve.keys[1].value, 9.);

    assert_eq!(curve.move_key(0, 3.), Some(2));
    let times: Vec<f32> = curve.keys.iter().map(|k| k.time).collect();
    assert_eq!(times, [1., 2., 3.]);
    // Moving onto another key replaces it.
    assert_eq!(curve.move_key(0, 2.), Some(0));
    assert_eq!(curve.keys.len(), 2);
    assert_eq!(curve.keys[0].value, 9.);

    assert!(curve.set_value(1, 5.));
    assert_eq!(curve.keys[1].value, 5.);
    assert!(curve.remove(0).is_some());
    assert!(curve.remove(4).is_none());
    assert!(!curve.set_value(3, 0.));
}

#[test]
fn right_tangent_needs_a_cubic_key() {
    let mut curve = FloatCurve::new(vec![key(0., 0., Interpolation::Linear)]);
    curve.set_tangents(0, Some(1.), Some(2.));
    assert_eq!(curve.keys[0].left_tangent, Some(1.));
    assert_eq!(curve.keys[0].right_tangent, None);
    curve.set_interpolation(0, Interpolation::Cubic);
    curve.set_tangents(0, Some(1.), Some(2.));
    assert_eq!(curve.keys[0].right_tangent, Some(2.));
    curve.set_interpolation(0, Interpolation::Constant);
    assert_eq!(curve.keys[0].right_tangent, None);
    assert_eq!(curve.keys[0].left_tangent, Some(1.));
}

#[test]
fn generate_curve_bakes_bounce_between_two_keys() {
    let mut curve = FloatCurve::new(vec![
        key(0., 0., Interpolation::Cubic),
        key(0.5, 7., Interpolation::Linear),
        key(1., 10., Interpolation::Linear),
    ]);
    assert!(curve.generate(0, 2, Style::Bounce, Direction::Out, 30.));
    assert_eq!(curve.keys.len(), 31);
    assert!(curve
        .keys
        .iter()
        .all(|k| k.interpolation == Interpolation::Linear));
    assert_eq!(curve.keys.last().unwrap().value, 10.);
    for k in &curve.keys {
        let want = 10. * ease(Style::Bounce, Direction::Out, k.time);
        assert!((k.value - want).abs() < 1e-4);
    }
    assert!(!curve.generate(2, 1, Style::Bounce, Direction::Out, 30.));
}

#[test]
fn range_includes_cubic_overshoot() {
    let curve = FloatCurve::new(vec![
        cubic(0., 0., None, Some(10.)),
        cubic(1., 1., Some(10.), None),
    ]);
    let (lo, hi) = curve.range().unwrap();
    assert!(lo < 0. && hi > 1., "{lo} {hi}");
    assert_eq!(FloatCurve::default().range(), None);
}

fn explicit(order: RotationOrder, a: Vec3) -> Quat {
    let (x, y, z) = (
        Quat::from_rotation_x(a.x),
        Quat::from_rotation_y(a.y),
        Quat::from_rotation_z(a.z),
    );
    match order {
        RotationOrder::XYZ => x * y * z,
        RotationOrder::XZY => x * z * y,
        RotationOrder::YZX => y * z * x,
        RotationOrder::YXZ => y * x * z,
        RotationOrder::ZXY => z * x * y,
        RotationOrder::ZYX => z * y * x,
    }
}

fn same_rotation(a: Quat, b: Quat) -> bool {
    a.angle_between(b) < 1e-4
}

#[test]
fn rotation_order_values_and_products() {
    for (i, order) in RotationOrder::ALL.into_iter().enumerate() {
        assert_eq!(order.value(), i as u32);
        assert_eq!(RotationOrder::from_value(order.value()), order);
    }
    let a = Vec3::new(0.3, -0.7, 1.1);
    for order in RotationOrder::ALL {
        assert!(
            same_rotation(order.compose(a), explicit(order, a)),
            "{order:?}"
        );
    }
    let xyz = RotationOrder::XYZ.compose(a);
    assert!(same_rotation(xyz, Frame::angles(a.x, a.y, a.z).rotation));
}

#[test]
fn euler_round_trips_in_every_order() {
    let samples = [
        Vec3::new(0.3, -0.7, 1.1),
        Vec3::new(-2.5, 1.2, 0.4),
        Vec3::new(1.0, 0.0, -3.0),
        Vec3::ZERO,
    ];
    for order in RotationOrder::ALL {
        for a in samples {
            let q = order.compose(a);
            let back = order.decompose(q);
            assert!(same_rotation(order.compose(back), q), "{order:?} {a}");
            assert!(back.abs().max_element() <= PI + 1e-5);
            // With the original as the reference, unwrapping recovers it.
            assert!(order.unwrap(back, a).abs_diff_eq(a, 1e-3), "{order:?} {a}");
        }
    }
}

#[test]
fn unwrap_avoids_turn_jumps_and_flips() {
    for order in RotationOrder::ALL {
        let near = Vec3::new(3.0, 0.2, -0.1);
        let wrapped = Vec3::new(3.2 - TAU, 0.2, -0.1);
        assert!(order
            .unwrap(wrapped, near)
            .abs_diff_eq(Vec3::new(3.2, 0.2, -0.1), 1e-4));
        // The same orientation read through the sequence's other solution.
        let a = Vec3::new(0.1, 1.4, 0.2);
        let q = order.compose(a);
        let read = order.decompose(q);
        let mid_axis = match order {
            RotationOrder::XYZ | RotationOrder::ZYX => 1,
            RotationOrder::XZY | RotationOrder::YZX => 2,
            RotationOrder::YXZ | RotationOrder::ZXY => 0,
        };
        let mut flipped = read.to_array();
        for (i, v) in flipped.iter_mut().enumerate() {
            *v = if i == mid_axis { PI - *v } else { *v + PI };
        }
        let flipped = Vec3::from_array(flipped);
        assert!(same_rotation(order.compose(flipped), q), "{order:?}");
        let near = order.unwrap(read, a);
        assert!(
            order.unwrap(flipped, near).abs_diff_eq(near, 1e-3),
            "{order:?}"
        );
    }
}

#[test]
fn joint_curves_eval_and_channels() {
    let mut joint = JointCurves::default();
    joint
        .curve_mut(Channel::PositionY)
        .insert(key(0., 2., Interpolation::Linear));
    joint
        .curve_mut(Channel::RotationZ)
        .insert(key(0., 0., Interpolation::Linear));
    joint
        .curve_mut(Channel::RotationZ)
        .insert(key(1., 1., Interpolation::Linear));
    assert_eq!(joint.curve(Channel::RotationZ).keys.len(), 2);
    assert!(Channel::RotationX.is_rotation() && !Channel::PositionZ.is_rotation());
    let frame = joint.eval(0.5);
    assert!(frame.position.abs_diff_eq(Vec3::new(0., 2., 0.), 1e-6));
    assert!(same_rotation(frame.rotation, Quat::from_rotation_z(0.5)));

    let ranges = value_range(&joint);
    assert_eq!(ranges.position, Some((2., 2.)));
    assert_eq!(ranges.rotation, Some((0., 1.)));
    assert_eq!(value_range(&JointCurves::default()), Ranges::default());
}

#[test]
fn curve_clip_wraps_and_skips_empty_joints() {
    let mut clip = CurveClip::default();
    let mut joint = JointCurves::default();
    joint.position[0] = FloatCurve::new(vec![
        key(0., 0., Interpolation::Linear),
        key(2., 4., Interpolation::Linear),
    ]);
    clip.joints.insert("Head".into(), joint);
    clip.joints.insert("Empty".into(), JointCurves::default());
    assert_eq!(clip.length(), 2.);
    assert_eq!(clip.sample(5.)["Head"].position.x, 4.);
    clip.looped = true;
    let looped = clip.sample(3.);
    assert!((looped["Head"].position.x - 2.).abs() < 1e-6);
    assert!(!looped.contains_key("Empty"));
}
