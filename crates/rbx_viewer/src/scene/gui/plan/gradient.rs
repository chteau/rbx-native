//! `UIGradient`: a colour and transparency ramp multiplied into everything
//! its parent paints, its geometry kept in the parent's own units until the
//! pixel box is known.

use std::collections::BTreeMap;

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref,
    Variant, WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use super::modifiers;
use super::props::{enum_of, flag, float, vector2};
use crate::scene::gui::style::Styled;

const CLASS: &str = "UIGradient";

/// `Enum.GradientType`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GradientKind {
    Linear,
    Radial,
    Conical,
}

/// `Enum.GradientTileMode`, in ordinal order: what fills the part of the box
/// the ramp does not reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tile {
    Clamp,
    Repeat,
    Mirror,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::scene::gui) struct Gradient {
    /// Kept in the sequence's own sRGB values: Roblox interpolates a
    /// `ColorSequence` there, so the ramp is baked in that space and only
    /// linearized afterwards (see `renderer::gui::gradient`).
    pub(in crate::scene::gui) color: ColorSequence,
    pub(in crate::scene::gui) transparency: NumberSequence,
    /// Degrees clockwise from the left-to-right ramp.
    pub(in crate::scene::gui) rotation: f32,
    /// A translation from the parent's centre in units of the parent's own
    /// size, per the docs: `(1, 0)` moves the ramp one parent width right.
    pub(in crate::scene::gui) offset: [f32; 2],
    pub(in crate::scene::gui) scale: f32,
    pub(in crate::scene::gui) kind: GradientKind,
    pub(in crate::scene::gui) tile: Tile,
}

/// The floor the docs give `Scale`, "to avoid degenerate gradients".
const MIN_SCALE: f32 = 0.001;

/// `Scale`, 1 where absent. Roblox clamps the property to [`MIN_SCALE`] on
/// write, so a zero can only be a class default filled in by a serializer
/// that does not know the property — read as unset rather than as the
/// thousandfold ramp the clamp would make of it.
fn scale(properties: &BTreeMap<String, Variant>) -> f32 {
    match float(properties, "Scale", 1.0) {
        value if value > 0.0 => value.max(MIN_SCALE),
        _ => 1.0,
    }
}

/// The first *enabled* `UIGradient` among `children`.
///
/// A missing `Color` is a flat white ramp and a missing `Transparency` a
/// flat opaque one: both multiply into the parent as a no-op, which is what
/// a fresh `UIGradient` shows in Studio.
pub(super) fn read(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    children: &[Ref],
) -> Option<Gradient> {
    let instance = modifiers(dom, database, children, CLASS)
        .find(|instance| flag(styles.properties_of(instance), "Enabled", true))?;
    let properties = styles.properties_of(instance);
    let color = match properties.get("Color") {
        Some(Variant::ColorSequence(sequence)) if !sequence.keypoints.is_empty() => {
            sequence.clone()
        }
        _ => ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: Color3Data {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
                envelope: 0.0,
            }],
        },
    };
    let transparency = match properties.get("Transparency") {
        Some(Variant::NumberSequence(sequence)) if !sequence.keypoints.is_empty() => {
            sequence.clone()
        }
        _ => NumberSequence {
            keypoints: vec![NumberSequenceKeypoint {
                time: 0.0,
                value: 0.0,
                envelope: 0.0,
            }],
        },
    };
    Some(Gradient {
        color,
        transparency,
        rotation: float(properties, "Rotation", 0.0),
        offset: vector2(properties, "Offset"),
        scale: scale(properties),
        kind: match enum_of(properties, "Type", 0) {
            1 => GradientKind::Radial,
            2 => GradientKind::Conical,
            _ => GradientKind::Linear,
        },
        tile: match enum_of(properties, "TileMode", 0) {
            1 => Tile::Repeat,
            2 => Tile::Mirror,
            _ => Tile::Clamp,
        },
    })
}
