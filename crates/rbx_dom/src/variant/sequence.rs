//! Keyframed and ranged property value types (NumberSequence, ColorSequence, NumberRange).

use super::geometry::Color3Data;

/// One point of a `NumberSequence`.
///
/// `envelope` is the symmetric random spread applied around `value` at render time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberSequenceKeypoint {
    pub time: f32,
    pub value: f32,
    pub envelope: f32,
}

/// A curve of scalar values indexed by normalized time.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberSequence {
    pub keypoints: Vec<NumberSequenceKeypoint>,
}

/// One point of a `ColorSequence`.
///
/// The `envelope` field is serialized by Roblox but has no effect in the engine;
/// it is kept so a value survives a decode/encode round trip unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorSequenceKeypoint {
    pub time: f32,
    pub color: Color3Data,
    pub envelope: f32,
}

/// A gradient of colors indexed by normalized time.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSequence {
    pub keypoints: Vec<ColorSequenceKeypoint>,
}

/// An inclusive range of scalar values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    pub min: f32,
    pub max: f32,
}
