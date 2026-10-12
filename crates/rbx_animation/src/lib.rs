//! Animation for Roblox rigs, independent of any UI or renderer: reading and
//! writing `KeyframeSequence`s, finding a rig's joints (`Motor6D` or
//! `AnimationConstraint`), sampling a clip with Roblox's pose easing,
//! blending tracks the way an `Animator` does, and evaluating animation
//! graphs.

pub mod animator;
pub mod clip;
pub mod curve;
pub mod easing;
pub mod facs;
pub mod frame;
pub mod graph;
pub mod ik;
pub mod import;
pub mod rig;
pub mod sample;
pub mod sequence;

#[cfg(test)]
mod fixture;

pub use clip::{Clip, Keyframe, Marker, PoseKey, Priority};
pub use easing::{ease, Direction, Style};
pub use frame::Frame;
pub use rig::{Joint, JointKind, Rig};
