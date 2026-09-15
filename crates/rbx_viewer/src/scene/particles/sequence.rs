//! Evaluates `NumberSequence`/`ColorSequence` at a normalized time, the way
//! `ParticleEmitter.Size`/`Transparency`/`Color` are read over a particle's age.
//!
//! Keypoint `envelope` (a per-keypoint random spread) is not applied: it is a
//! per-particle-per-keypoint detail that has no visible effect on the steady
//! state this viewer renders, so it is left at its base value — a documented
//! TODO rather than a silent approximation.

use rbx_dom::{ColorSequence, NumberSequence};

use crate::scene::srgb_to_linear;

/// Linearly interpolates `seq` at `t` (clamped to `[0, 1]`), the same rule
/// Roblox uses for a particle's age over its lifetime.
pub(crate) fn eval_number(seq: &NumberSequence, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let keypoints = &seq.keypoints;
    let Some(first) = keypoints.first() else {
        return 0.0;
    };
    if keypoints.len() == 1 || t <= first.time {
        return first.value;
    }
    for pair in keypoints.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.time {
            let span = b.time - a.time;
            let f = if span > 0.0 { (t - a.time) / span } else { 0.0 };
            return a.value + (b.value - a.value) * f;
        }
    }
    keypoints.last().map_or(0.0, |last| last.value)
}

/// Same rule as [`eval_number`], but for a colour gradient.
///
/// Interpolated in the sequence's own (sRGB) space, like Roblox's own
/// `ColorSequence`, then linearized once at the end — see [`srgb_to_linear`] —
/// rather than linearizing each keypoint first, which would shift the gradient.
pub(crate) fn eval_color(seq: &ColorSequence, t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let keypoints = &seq.keypoints;
    let Some(first) = keypoints.first() else {
        return [0.0; 3];
    };
    let raw = |c: rbx_dom::Color3Data| [c.r, c.g, c.b];
    let raw = if keypoints.len() == 1 || t <= first.time {
        raw(first.color)
    } else {
        keypoints
            .windows(2)
            .find_map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                if t > b.time {
                    return None;
                }
                let span = b.time - a.time;
                let f = if span > 0.0 { (t - a.time) / span } else { 0.0 };
                let (ca, cb) = (raw(a.color), raw(b.color));
                Some([
                    ca[0] + (cb[0] - ca[0]) * f,
                    ca[1] + (cb[1] - ca[1]) * f,
                    ca[2] + (cb[2] - ca[2]) * f,
                ])
            })
            .unwrap_or_else(|| raw(keypoints.last().expect("checked non-empty above").color))
    };
    [
        srgb_to_linear(raw[0]),
        srgb_to_linear(raw[1]),
        srgb_to_linear(raw[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::{Color3Data, ColorSequenceKeypoint, NumberSequenceKeypoint};

    fn numbers(points: &[(f32, f32)]) -> NumberSequence {
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

    fn colors(points: &[(f32, [f32; 3])]) -> ColorSequence {
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

    #[test]
    fn a_two_keypoint_sequence_interpolates_linearly_between_them() {
        let seq = numbers(&[(0.0, 0.0), (1.0, 10.0)]);
        assert_eq!(eval_number(&seq, 0.0), 0.0);
        assert_eq!(eval_number(&seq, 0.5), 5.0);
        assert_eq!(eval_number(&seq, 1.0), 10.0);
    }

    #[test]
    fn time_outside_zero_one_clamps_to_the_nearest_keypoint() {
        let seq = numbers(&[(0.0, 2.0), (1.0, 8.0)]);
        assert_eq!(eval_number(&seq, -1.0), 2.0);
        assert_eq!(eval_number(&seq, 2.0), 8.0);
    }

    #[test]
    fn a_middle_keypoint_is_reached_exactly_at_its_own_time() {
        let seq = numbers(&[(0.0, 0.0), (0.25, 100.0), (1.0, 0.0)]);
        assert_eq!(eval_number(&seq, 0.25), 100.0);
        assert_eq!(eval_number(&seq, 0.125), 50.0);
    }

    #[test]
    fn a_single_keypoint_sequence_is_constant() {
        let seq = numbers(&[(0.0, 3.0)]);
        assert_eq!(eval_number(&seq, 0.0), 3.0);
        assert_eq!(eval_number(&seq, 0.9), 3.0);
    }

    #[test]
    fn an_empty_sequence_evaluates_to_zero() {
        let seq = NumberSequence { keypoints: vec![] };
        assert_eq!(eval_number(&seq, 0.5), 0.0);
    }

    #[test]
    fn color_interpolates_per_channel_and_linearizes_srgb() {
        let seq = colors(&[(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])]);
        assert_eq!(eval_color(&seq, 0.0), [0.0, 0.0, 0.0]);
        assert_eq!(eval_color(&seq, 1.0), [1.0, 1.0, 1.0]);
        // Halfway in sRGB space is not halfway in linear space.
        let mid = eval_color(&seq, 0.5);
        assert!(mid[0] > 0.0 && mid[0] < 0.5);
    }
}
