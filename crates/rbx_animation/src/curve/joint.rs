//! One joint's six channels (a `Vector3Curve` "Position" and an
//! `EulerRotationCurve` "Rotation") and the whole clip built from them.

use std::collections::BTreeMap;

use glam::Vec3;

use super::{FloatCurve, RotationOrder};
use crate::clip::{Marker, Priority};
use crate::frame::Frame;

/// One `FloatCurve` of a joint, in the order [`JointCurves::curve`] indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    PositionX,
    PositionY,
    PositionZ,
    RotationX,
    RotationY,
    RotationZ,
}

impl Channel {
    pub const ALL: [Channel; 6] = [
        Channel::PositionX,
        Channel::PositionY,
        Channel::PositionZ,
        Channel::RotationX,
        Channel::RotationY,
        Channel::RotationZ,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Rotation channels are in radians and plot against their own ruler.
    pub fn is_rotation(self) -> bool {
        self.index() >= 3
    }

    /// The axis, which is also the curve's `Name` under its parent.
    pub fn axis(self) -> usize {
        self.index() % 3
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::PositionX => "Position X",
            Channel::PositionY => "Position Y",
            Channel::PositionZ => "Position Z",
            Channel::RotationX => "Rotation X",
            Channel::RotationY => "Rotation Y",
            Channel::RotationZ => "Rotation Z",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JointCurves {
    pub position: [FloatCurve; 3],
    /// Radians about X, Y and Z, composed in `order`.
    pub rotation: [FloatCurve; 3],
    pub order: RotationOrder,
}

impl JointCurves {
    pub fn curve(&self, channel: Channel) -> &FloatCurve {
        let group = if channel.is_rotation() {
            &self.rotation
        } else {
            &self.position
        };
        &group[channel.axis()]
    }

    pub fn curve_mut(&mut self, channel: Channel) -> &mut FloatCurve {
        let group = if channel.is_rotation() {
            &mut self.rotation
        } else {
            &mut self.position
        };
        &mut group[channel.axis()]
    }

    pub fn is_empty(&self) -> bool {
        self.position
            .iter()
            .chain(&self.rotation)
            .all(FloatCurve::is_empty)
    }

    pub fn length(&self) -> f32 {
        self.position
            .iter()
            .chain(&self.rotation)
            .map(FloatCurve::length)
            .fold(0., f32::max)
    }

    /// `CFrame.new(position) * rotation`. A channel with no keys reads as 0,
    /// which is what `GetRotationAtTime` does for rotation; doing the same
    /// for position keeps a partly keyed joint at its rest offset.
    pub fn eval(&self, time: f32) -> Frame {
        let read = |curves: &[FloatCurve; 3]| {
            Vec3::from_array(curves.each_ref().map(|c| c.eval(time).unwrap_or(0.)))
        };
        Frame::new(
            read(&self.position),
            self.order.compose(read(&self.rotation)),
        )
    }
}

/// The value span of each ruler, for fitting the graph's vertical axis.
/// `None` where that group has no keys.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ranges {
    pub position: Option<(f32, f32)>,
    pub rotation: Option<(f32, f32)>,
}

pub fn value_range(curves: &JointCurves) -> Ranges {
    let group = |c: &[FloatCurve; 3]| {
        c.iter()
            .filter_map(FloatCurve::range)
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    };
    Ranges {
        position: group(&curves.position),
        rotation: group(&curves.rotation),
    }
}

/// A `CurveAnimation`: an `AnimationClip`'s settings and a set of curves per
/// joint, keyed by the joint's `Part1` name as a `Pose` is.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveClip {
    pub name: String,
    pub looped: bool,
    pub priority: Priority,
    pub joints: BTreeMap<String, JointCurves>,
    /// Sorted by time.
    pub markers: Vec<(f32, Marker)>,
}

impl Default for CurveClip {
    fn default() -> Self {
        CurveClip {
            name: "Animation".into(),
            looped: false,
            priority: Priority::Action,
            joints: BTreeMap::new(),
            markers: Vec::new(),
        }
    }
}

impl CurveClip {
    /// The latest key or marker: a marker past the last key still has to be
    /// reached, so it lengthens the clip as it would a `KeyframeSequence`.
    pub fn length(&self) -> f32 {
        self.joints
            .values()
            .map(JointCurves::length)
            .chain(self.markers.iter().map(|(t, _)| *t))
            .fold(0., f32::max)
    }

    /// Wraps a looped clip and clamps any other, as [`crate::sample::wrap`]
    /// does for a [`Clip`](crate::clip::Clip).
    pub fn wrap(&self, time: f32) -> f32 {
        let length = self.length();
        if length <= 0. {
            0.
        } else if self.looped {
            time.rem_euclid(length)
        } else {
            time.clamp(0., length)
        }
    }

    /// Every joint with at least one key, at `time` (wrapped or clamped here).
    pub fn sample(&self, time: f32) -> BTreeMap<String, Frame> {
        let time = self.wrap(time);
        self.joints
            .iter()
            .filter(|(_, j)| !j.is_empty())
            .map(|(name, j)| (name.clone(), j.eval(time)))
            .collect()
    }

    pub fn add_marker(&mut self, time: f32, marker: Marker) {
        let i = self.markers.partition_point(|(t, _)| *t <= time);
        self.markers.insert(i, (time, marker));
    }
}
