//! Property readers shared by the plans a [`Scene`](super::Scene) is built
//! from: every one of them reads the same `BTreeMap<String, Variant>` a DOM
//! instance carries, and a `Fire`, a `SelectionBox` and a `ParticleEmitter`
//! all read the same kinds of value out of it.
//!
//! `pub(super)` here is `pub(in crate::scene)`: every plan module is a
//! descendant of `scene`, and nothing outside it reads raw properties.

use std::collections::BTreeMap;

use glam::Vec3;
use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint,
    Variant,
};

use glam::Mat4;

use crate::textures::NormalId;

/// One instance's property table, as [`rbx_dom`] stores it.
pub(super) type Properties = BTreeMap<String, Variant>;

pub(super) fn bool_or(properties: &Properties, key: &str, default: bool) -> bool {
    match properties.get(key) {
        Some(Variant::Bool(value)) => *value,
        _ => default,
    }
}

pub(super) fn float_or(properties: &Properties, key: &str, default: f32) -> f32 {
    match properties.get(key) {
        Some(Variant::Float32(value)) if value.is_finite() => *value,
        Some(Variant::Float64(value)) if value.is_finite() => *value as f32,
        _ => default,
    }
}

/// The first of `keys` the instance actually carries, for a property Roblox
/// saves under more than one spelling — `Fire.Size` and the legacy lowercase
/// `Fire.size`, both of which the API dump lists.
pub(super) fn float_of_any(properties: &Properties, keys: &[&str], default: f32) -> f32 {
    keys.iter()
        .find(|key| properties.contains_key(**key))
        .map(|key| float_or(properties, key, default))
        .unwrap_or(default)
}

pub(super) fn number_range_or(
    properties: &Properties,
    key: &str,
    default: (f32, f32),
) -> (f32, f32) {
    match properties.get(key) {
        Some(Variant::NumberRange(range)) => (range.min, range.max),
        _ => default,
    }
}

pub(super) fn vector2_or(properties: &Properties, key: &str, default: (f32, f32)) -> (f32, f32) {
    match properties.get(key) {
        Some(Variant::Vector2(v)) => (v.x, v.y),
        _ => default,
    }
}

pub(super) fn vector3_or(properties: &Properties, key: &str, default: Vec3) -> Vec3 {
    match properties.get(key) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => default,
    }
}

/// A plain `Color3` property, as the `[f32; 3]` the sequence builders below
/// take. The first of `keys` present wins, for a colour Roblox exposes under
/// two names — `Sparkles.SparkleColor` and its deprecated `Sparkles.Color`.
pub(super) fn color3_of_any(properties: &Properties, keys: &[&str], default: [f32; 3]) -> [f32; 3] {
    for key in keys {
        if let Some(Variant::Color3(color)) = properties.get(*key) {
            return [color.r, color.g, color.b];
        }
    }
    default
}

pub(super) fn number_sequence_or(
    properties: &Properties,
    key: &str,
    default: &[(f32, f32)],
) -> NumberSequence {
    match properties.get(key) {
        Some(Variant::NumberSequence(sequence)) => sequence.clone(),
        _ => number_sequence(default),
    }
}

/// A `NumberSequence` from plain `(time, value)` pairs — what an effect with
/// no sequence property of its own (a `Fire`, a `Smoke`) is given instead.
pub(super) fn number_sequence(points: &[(f32, f32)]) -> NumberSequence {
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

pub(super) fn color_sequence_or(
    properties: &Properties,
    key: &str,
    default: [f32; 3],
) -> ColorSequence {
    match properties.get(key) {
        Some(Variant::ColorSequence(sequence)) => sequence.clone(),
        _ => flat_color(default),
    }
}

/// One colour held for the whole lifetime — a `Fire`'s `Color` or a `Smoke`'s,
/// neither of which is a sequence.
pub(super) fn flat_color(color: [f32; 3]) -> ColorSequence {
    color_sequence(&[(0.0, color), (1.0, color)])
}

pub(super) fn color_sequence(points: &[(f32, [f32; 3])]) -> ColorSequence {
    ColorSequence {
        keypoints: points
            .iter()
            .map(|&(time, color)| ColorSequenceKeypoint {
                time,
                color: Color3Data {
                    r: color[0],
                    g: color[1],
                    b: color[2],
                },
                envelope: 0.0,
            })
            .collect(),
    }
}

pub(super) fn normal_id_or(properties: &Properties, key: &str, default: NormalId) -> NormalId {
    match properties.get(key) {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw).unwrap_or(default),
        _ => default,
    }
}

pub(super) fn int_or(properties: &Properties, key: &str, default: i32) -> i32 {
    match properties.get(key) {
        Some(Variant::Int32(value)) => *value,
        Some(Variant::Int64(value)) => *value as i32,
        _ => default,
    }
}

/// A `CFrame` property as a matrix, identity when absent.
pub(super) fn cframe_or(properties: &Properties, key: &str) -> Mat4 {
    match properties.get(key) {
        Some(Variant::CFrame(cframe)) => super::cframe_matrix(cframe),
        _ => Mat4::IDENTITY,
    }
}

/// A `Color3` property, linearized the way every other colour a `Scene`
/// hands the renderer already is. `default` is given in the same sRGB the
/// file carries, not in linear light.
pub(super) fn linear_color_or(properties: &Properties, key: &str, default: [f32; 3]) -> [f32; 3] {
    color3_of_any(properties, &[key], default).map(super::srgb_to_linear)
}
