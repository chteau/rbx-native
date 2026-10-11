//! Evaluates a [`Clip`] at a time: each joint's `Transform`, interpolated
//! between the two keyframes around that time that pose it.
//!
//! A keyframe that does not pose a joint is invisible to that joint. Its
//! motion runs from its previous pose to its next one, with the easing of the
//! earlier pose, which is the one that `Pose.EasingStyle` documents as
//! governing the way to the next keyframe. Before a joint's first key it
//! holds that key, and after its last key it holds that one too.

use std::collections::BTreeMap;

use crate::clip::{Clip, PoseKey};
use crate::easing::ease;
use crate::frame::Frame;

/// One joint's sampled pose: the `Transform` and how much of it applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampled {
    pub transform: Frame,
    pub weight: f32,
}

/// Where `time` lands inside a clip that may loop. A looped clip wraps;
/// anything else clamps to its ends.
pub fn wrap(clip: &Clip, time: f32) -> f32 {
    let length = clip.length();
    if length <= 0. {
        return 0.;
    }
    if clip.looped {
        time.rem_euclid(length)
    } else {
        time.clamp(0., length)
    }
}

fn between(from: (f32, &PoseKey), to: (f32, &PoseKey), time: f32) -> Sampled {
    let span = to.0 - from.0;
    let t = if span > 0. {
        (time - from.0) / span
    } else {
        1.
    };
    let alpha = ease(from.1.style, from.1.direction, t);
    Sampled {
        transform: from.1.cframe.lerp(&to.1.cframe, alpha),
        weight: from.1.weight + (to.1.weight - from.1.weight) * alpha.clamp(0., 1.),
    }
}

/// One joint at `time` (already wrapped), or `None` when the clip never
/// poses it.
pub fn sample_joint(clip: &Clip, joint: &str, time: f32) -> Option<Sampled> {
    let mut previous: Option<(f32, &PoseKey)> = None;
    for (t, key) in clip.track(joint) {
        if t > time {
            return Some(match previous {
                Some(from) => between(from, (t, key), time),
                None => Sampled {
                    transform: key.cframe,
                    weight: key.weight,
                },
            });
        }
        previous = Some((t, key));
    }
    previous.map(|(_, key)| Sampled {
        transform: key.cframe,
        weight: key.weight,
    })
}

/// Every joint the clip poses, at `time` (wrapped or clamped here).
pub fn sample(clip: &Clip, time: f32) -> BTreeMap<String, Sampled> {
    let time = wrap(clip, time);
    clip.joints()
        .into_iter()
        .filter_map(|joint| sample_joint(clip, &joint, time).map(|s| (joint, s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::easing::{Direction, Style};

    fn at(x: f32) -> PoseKey {
        PoseKey::new(Frame::at(Vec3::new(x, 0., 0.)))
    }

    fn x(s: Option<Sampled>) -> f32 {
        s.unwrap().transform.position.x
    }

    #[test]
    fn interpolates_and_holds_the_ends() {
        let mut clip = Clip::default();
        clip.set_pose(1., "Head", at(0.));
        clip.set_pose(3., "Head", at(10.));
        assert_eq!(x(sample_joint(&clip, "Head", 0.)), 0.);
        assert!((x(sample_joint(&clip, "Head", 2.)) - 5.).abs() < 1e-5);
        assert_eq!(x(sample_joint(&clip, "Head", 4.)), 10.);
        assert!(sample_joint(&clip, "Torso", 1.).is_none());
    }

    #[test]
    fn keyframes_without_the_joint_are_skipped() {
        let mut clip = Clip::default();
        clip.set_pose(0., "Head", at(0.));
        clip.set_pose(1., "Torso", at(99.));
        clip.set_pose(2., "Head", at(4.));
        assert!((x(sample_joint(&clip, "Head", 1.)) - 2.).abs() < 1e-5);
    }

    #[test]
    fn the_earlier_pose_easing_governs_the_segment() {
        let mut clip = Clip::default();
        let mut first = at(0.);
        first.style = Style::CubicV2;
        first.direction = Direction::In;
        clip.set_pose(0., "Head", first);
        clip.set_pose(1., "Head", at(8.));
        assert!((x(sample_joint(&clip, "Head", 0.5)) - 1.).abs() < 1e-5);

        let mut hold = at(0.);
        hold.style = Style::Constant;
        hold.direction = Direction::Out;
        clip.set_pose(0., "Head", hold);
        assert_eq!(x(sample_joint(&clip, "Head", 0.9)), 0.);
    }

    #[test]
    fn looped_clips_wrap_and_others_clamp() {
        let mut clip = Clip::default();
        clip.set_pose(0., "Head", at(0.));
        clip.set_pose(2., "Head", at(2.));
        assert_eq!(wrap(&clip, 3.), 2.);
        clip.looped = true;
        assert!((wrap(&clip, 3.) - 1.).abs() < 1e-6);
        assert!((wrap(&clip, -0.5) - 1.5).abs() < 1e-6);
        let all = sample(&clip, 3.);
        assert!((all["Head"].transform.position.x - 1.).abs() < 1e-5);
    }

    #[test]
    fn weight_interpolates_with_the_pose() {
        let mut clip = Clip::default();
        let mut light = at(0.);
        light.weight = 0.;
        clip.set_pose(0., "Head", light);
        clip.set_pose(1., "Head", at(0.));
        assert!((sample_joint(&clip, "Head", 0.25).unwrap().weight - 0.25).abs() < 1e-5);
    }
}
