//! The in-memory form of a `KeyframeSequence`: keyframes on a timeline, each
//! holding a pose for some of the rig's joints. This is the shape Roblox's
//! own instances have, kept as-is so that a clip read from a place writes
//! back the same way; [`Clip::track`] gives the per-joint view the editor's
//! dope sheet and the sampler want.

use std::collections::BTreeMap;

use crate::easing::{Direction, Style};
use crate::frame::Frame;

/// `Enum.AnimationPriority`. Values are the enum's own integers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Idle,
    Movement,
    Action,
    Action2,
    Action3,
    Action4,
    #[default]
    Core,
}

impl Priority {
    pub const ALL: [Priority; 7] = [
        Priority::Idle,
        Priority::Movement,
        Priority::Action,
        Priority::Action2,
        Priority::Action3,
        Priority::Action4,
        Priority::Core,
    ];

    pub fn value(self) -> u32 {
        match self {
            Priority::Idle => 0,
            Priority::Movement => 1,
            Priority::Action => 2,
            Priority::Action2 => 3,
            Priority::Action3 => 4,
            Priority::Action4 => 5,
            Priority::Core => 1000,
        }
    }

    pub fn from_value(value: u32) -> Priority {
        Priority::ALL
            .into_iter()
            .find(|p| p.value() == value)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Idle => "Idle",
            Priority::Movement => "Movement",
            Priority::Action => "Action",
            Priority::Action2 => "Action2",
            Priority::Action3 => "Action3",
            Priority::Action4 => "Action4",
            Priority::Core => "Core",
        }
    }
}

/// One joint's key: a `Pose`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseKey {
    /// The joint's `Transform` at this key.
    pub cframe: Frame,
    /// How the joint travels from this key to the next one that poses it.
    pub style: Style,
    pub direction: Direction,
    pub weight: f32,
}

impl PoseKey {
    pub fn new(cframe: Frame) -> PoseKey {
        PoseKey {
            cframe,
            style: Style::default(),
            direction: Direction::default(),
            weight: 1.,
        }
    }
}

/// A `KeyframeMarker`: a named event an `AnimationTrack` fires through
/// `GetMarkerReachedSignal`.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    pub time: f32,
    /// A `Keyframe`'s own `Name`, which is what `KeyframeReached` reports;
    /// "Keyframe" is the class default and means unnamed.
    pub name: String,
    /// Keyed by the joint's `Part1` name, which is how a `Pose` addresses it.
    pub poses: BTreeMap<String, PoseKey>,
    pub markers: Vec<Marker>,
}

impl Keyframe {
    pub fn at(time: f32) -> Keyframe {
        Keyframe {
            time,
            name: "Keyframe".into(),
            poses: BTreeMap::new(),
            markers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    pub looped: bool,
    pub priority: Priority,
    /// Sorted by time, never two at the same time.
    pub keyframes: Vec<Keyframe>,
}

impl Default for Clip {
    fn default() -> Self {
        Clip {
            name: "Animation".into(),
            looped: false,
            priority: Priority::Action,
            keyframes: Vec::new(),
        }
    }
}

/// Two keyframe times closer than this are the same keyframe: well under one
/// frame at 240 fps, and above the noise a float time picks up on a round
/// trip through a file.
pub const TIME_EPSILON: f32 = 1e-4;

impl Clip {
    /// The time of the last keyframe: a `KeyframeSequence` has no length of
    /// its own.
    pub fn length(&self) -> f32 {
        self.keyframes.last().map_or(0., |k| k.time)
    }

    /// Every joint the clip poses at least once, in name order.
    pub fn joints(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .keyframes
            .iter()
            .flat_map(|k| k.poses.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The keys one joint has, in time order.
    pub fn track<'a>(&'a self, joint: &'a str) -> impl Iterator<Item = (f32, &'a PoseKey)> + 'a {
        self.keyframes
            .iter()
            .filter_map(move |k| k.poses.get(joint).map(|p| (k.time, p)))
    }

    pub fn keyframe_index(&self, time: f32) -> Option<usize> {
        self.keyframes
            .iter()
            .position(|k| (k.time - time).abs() < TIME_EPSILON)
    }

    /// The keyframe at `time`, made if none is there yet, kept in order.
    pub fn keyframe_mut(&mut self, time: f32) -> &mut Keyframe {
        let time = time.max(0.);
        let index = match self.keyframe_index(time) {
            Some(i) => i,
            None => {
                let i = self.keyframes.partition_point(|k| k.time < time);
                self.keyframes.insert(i, Keyframe::at(time));
                i
            }
        };
        &mut self.keyframes[index]
    }

    pub fn set_pose(&mut self, time: f32, joint: &str, key: PoseKey) {
        self.keyframe_mut(time).poses.insert(joint.to_owned(), key);
    }

    pub fn pose(&self, time: f32, joint: &str) -> Option<&PoseKey> {
        self.keyframes
            .get(self.keyframe_index(time)?)?
            .poses
            .get(joint)
    }

    pub fn pose_mut(&mut self, time: f32, joint: &str) -> Option<&mut PoseKey> {
        let i = self.keyframe_index(time)?;
        self.keyframes[i].poses.get_mut(joint)
    }

    /// Removes one joint's key; a keyframe left with no pose and no marker
    /// goes too, as it would in Roblox's editor.
    pub fn remove_pose(&mut self, time: f32, joint: &str) -> Option<PoseKey> {
        let i = self.keyframe_index(time)?;
        let removed = self.keyframes[i].poses.remove(joint);
        if self.keyframes[i].poses.is_empty() && self.keyframes[i].markers.is_empty() {
            self.keyframes.remove(i);
        }
        removed
    }

    pub fn remove_keyframe(&mut self, time: f32) -> Option<Keyframe> {
        let i = self.keyframe_index(time)?;
        Some(self.keyframes.remove(i))
    }

    /// Moves one joint's key to another time, merging into whatever keyframe
    /// is already there. Returns false when there was no key to move.
    pub fn move_pose(&mut self, from: f32, to: f32, joint: &str) -> bool {
        if (from - to).abs() < TIME_EPSILON {
            return self.pose(from, joint).is_some();
        }
        let Some(key) = self.remove_pose(from, joint) else {
            return false;
        };
        self.set_pose(to, joint, key);
        true
    }

    /// Moves a whole keyframe (every pose and marker on it).
    pub fn move_keyframe(&mut self, from: f32, to: f32) -> bool {
        let Some(mut moved) = self.remove_keyframe(from) else {
            return false;
        };
        let target = self.keyframe_mut(to);
        target.poses.append(&mut moved.poses);
        target.markers.append(&mut moved.markers);
        if target.name == "Keyframe" {
            target.name = moved.name;
        }
        true
    }

    /// Scales every key's time by `factor`, the editor's "stretch".
    pub fn scale_time(&mut self, factor: f32) {
        if factor <= 0. {
            return;
        }
        for k in &mut self.keyframes {
            k.time *= factor;
        }
    }
}

#[cfg(test)]
mod tests;
