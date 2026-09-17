//! Typed reads of the `Variant`s a `GuiObject`'s properties hold, each with
//! the default Roblox itself falls back to when the property is absent — a
//! tree built in code, unlike a saved place, serializes nothing.

use std::collections::BTreeMap;

use rbx_dom::Variant;

use super::Span;
use crate::scene::srgb_to_linear;

pub(in crate::scene::gui) fn span(properties: &BTreeMap<String, Variant>, name: &str) -> Span {
    match properties.get(name) {
        Some(Variant::UDim2(value)) => Span {
            scale: [value.x.scale, value.y.scale],
            offset: [value.x.offset as f32, value.y.offset as f32],
        },
        _ => Span::default(),
    }
}

pub(in crate::scene::gui) fn vector2(
    properties: &BTreeMap<String, Variant>,
    name: &str,
) -> [f32; 2] {
    match properties.get(name) {
        Some(Variant::Vector2(value)) => [value.x, value.y],
        _ => [0.0, 0.0],
    }
}

/// A `Color3` linearized, since the display target re-encodes on write — same
/// reasoning as [`crate::scene::srgb_to_linear`]'s own callers.
pub(in crate::scene::gui) fn color(
    properties: &BTreeMap<String, Variant>,
    name: &str,
    default: [f32; 3],
) -> [f32; 3] {
    let raw = match properties.get(name) {
        Some(Variant::Color3(value)) => [value.r, value.g, value.b],
        Some(&Variant::Color3uint8 { r, g, b }) => {
            [r, g, b].map(|channel| f32::from(channel) / 255.0)
        }
        _ => default,
    };
    raw.map(srgb_to_linear)
}

/// `Rotation`, 0 degrees (unrotated) where the property is missing.
pub(in crate::scene::gui) fn degrees(properties: &BTreeMap<String, Variant>, name: &str) -> f32 {
    float(properties, name, 0.0)
}

/// A plain `float` property, e.g. `SliceScale`.
pub(in crate::scene::gui) fn float(
    properties: &BTreeMap<String, Variant>,
    name: &str,
    default: f32,
) -> f32 {
    match properties.get(name) {
        Some(Variant::Float32(value)) => *value,
        Some(Variant::Float64(value)) => *value as f32,
        _ => default,
    }
}

/// `1 - Transparency`, 1 (fully opaque) where the property is missing.
pub(in crate::scene::gui) fn alpha(properties: &BTreeMap<String, Variant>, name: &str) -> f32 {
    let transparency = match properties.get(name) {
        Some(Variant::Float32(value)) => *value,
        Some(Variant::Float64(value)) => *value as f32,
        _ => 0.0,
    };
    if transparency.is_finite() {
        1.0 - transparency.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

pub(in crate::scene::gui) fn flag(
    properties: &BTreeMap<String, Variant>,
    name: &str,
    default: bool,
) -> bool {
    match properties.get(name) {
        Some(&Variant::Bool(value)) => value,
        _ => default,
    }
}

pub(in crate::scene::gui) fn integer(
    properties: &BTreeMap<String, Variant>,
    name: &str,
    default: i32,
) -> i32 {
    match properties.get(name) {
        Some(&Variant::Int32(value)) => value,
        Some(&Variant::Float32(value)) => value as i32,
        _ => default,
    }
}

pub(in crate::scene::gui) fn enum_of(
    properties: &BTreeMap<String, Variant>,
    name: &str,
    default: u32,
) -> u32 {
    match properties.get(name) {
        Some(&Variant::Enum(value)) => value,
        _ => default,
    }
}

/// One `UDim` as its `(scale, offset)` pair, `None` where the property is
/// missing so a caller can tell "unset" from `UDim.new(0, 0)`.
pub(in crate::scene::gui) fn udim(
    properties: &BTreeMap<String, Variant>,
    name: &str,
) -> Option<(f32, f32)> {
    match properties.get(name) {
        Some(Variant::UDim(value)) => Some((value.scale, value.offset as f32)),
        _ => None,
    }
}
