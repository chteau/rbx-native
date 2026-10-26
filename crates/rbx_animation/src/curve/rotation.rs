//! A quaternion `RotationCurve` turned into the Euler channels the editor
//! edits. Studio moves between two keys by slerping with the fraction
//! `s(u)` of a Hermite segment from 0 to 1, whose end slopes are the left
//! key's right tangent and the right key's left tangent times the segment's
//! length (so a tangent is in fractions of the segment per second); a
//! linear key slerps by `u` itself. Measured with `GetValueAtTime` in
//! Studio: `s` matched to five places for 0.5 and 1 radian turns.
//!
//! Euler channels interpolated linearly only follow a slerp at their keys,
//! so every non-constant segment is keyed at [`FPS`] along the way.

use glam::Quat;

use super::values::{seconds, RawRotationKey};
use super::{bake_times, FloatCurve, FloatKey, Interpolation, RotationOrder};

const FPS: f32 = 30.;

fn hermite_fraction(u: f32, start_slope: f32, end_slope: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (3. * u2 - 2. * u3) + (u3 - 2. * u2 + u) * start_slope + (u3 - u2) * end_slope
}

fn quat(key: &RawRotationKey) -> Quat {
    Quat::from_array(key.rotation).normalize()
}

/// X, Y and Z curves composing in `RotationOrder::XYZ`.
pub(super) fn to_euler(keys: &[RawRotationKey]) -> [FloatCurve; 3] {
    let mut samples: Vec<(f32, Quat, Interpolation)> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let (t0, q0) = (seconds(key.ticks), quat(key));
        let interpolation = Interpolation::from_value(key.interpolation.into());
        let next = keys.get(i + 1);
        let Some(next) = next.filter(|_| interpolation != Interpolation::Constant) else {
            samples.push((t0, q0, interpolation));
            continue;
        };
        samples.push((t0, q0, Interpolation::Linear));
        let (t1, q1) = (seconds(next.ticks), quat(next));
        let span = t1 - t0;
        for t in bake_times(t0, t1, FPS) {
            let u = (t - t0) / span;
            let s = match interpolation {
                Interpolation::Cubic => hermite_fraction(u, key.right * span, next.left * span),
                _ => u,
            };
            samples.push((t, q0.slerp(q1, s), Interpolation::Linear));
        }
    }
    let order = RotationOrder::XYZ;
    let mut curves: [FloatCurve; 3] = Default::default();
    let mut previous = None;
    for (time, rotation, interpolation) in samples {
        let mut angles = order.decompose(rotation);
        if let Some(near) = previous {
            angles = order.unwrap(angles, near);
        }
        previous = Some(angles);
        for (curve, value) in curves.iter_mut().zip(angles.to_array()) {
            curve.insert(FloatKey::new(time, value, interpolation));
        }
    }
    curves
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seconds: f32, turn_y: f32, interpolation: u8, left: f32, right: f32) -> RawRotationKey {
        RawRotationKey {
            ticks: (seconds * 2400.) as u32,
            interpolation,
            rotation: Quat::from_rotation_y(turn_y).to_array(),
            left,
            right,
        }
    }

    #[test]
    fn cubic_segments_follow_what_studio_plays() {
        // `GetValueAtTime` at 0.25 s steps of a 1 radian turn about Y over
        // 2 s, for (right tangent of the first key, left of the second).
        let cases = [
            (
                (0., 0.),
                [0.04297, 0.15625, 0.31641, 0.5, 0.68359, 0.84375, 0.95703],
            ),
            (
                (2., 0.),
                [0.42578, 0.71875, 0.90234, 1.0, 1.03516, 1.03125, 1.01172],
            ),
            (
                (0., 3.),
                [-0.03906, -0.125, -0.21094, -0.25, -0.19531, 0., 0.38281],
            ),
        ];
        for ((right, left), studio) in cases {
            let curves = to_euler(&[key(0., 0., 2, 0., right), key(2., 1., 2, left, 0.)]);
            for (i, want) in studio.into_iter().enumerate() {
                let t = 0.25 * (i + 1) as f32;
                let got = curves[1].eval(t).unwrap();
                assert!(
                    (got - want).abs() < 3e-3,
                    "{right},{left} at {t}: {got} vs {want}"
                );
            }
        }
    }

    #[test]
    fn constant_holds_and_linear_slerps() {
        let curves = to_euler(&[
            key(0., 0., 1, 0., 0.),
            key(1., 1., 0, 0., 0.),
            key(2., 0., 0, 0., 0.),
        ]);
        assert!((curves[1].eval(0.5).unwrap() - 0.5).abs() < 1e-4);
        assert!((curves[1].eval(1.7).unwrap() - 1.).abs() < 1e-6);
        assert_eq!(curves[1].eval(2.).unwrap(), 0.);
    }
}
