//! Reads a `Beam` instance, resolves its two `Attachment` endpoints into
//! world space (see [`super::attachment`]) and folds `CurveSize0`/`CurveSize1`
//! into a [`Curve`], everything [`super::ribbon`]-equivalent GPU code needs
//! without touching the DOM again.

use std::collections::BTreeMap;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{
    ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref, Variant,
    WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use super::attachment::{self, ParentMap};
use super::curve::Curve;
use crate::scene::descendants;
use crate::textures::asset_uri;

const CLASS: &str = "Beam";

/// Studio's own defaults, so a `Beam` that omits a property lands where the
/// property sheet shows it rather than at zero.
const DEFAULT_WIDTH: f32 = 1.0;
const DEFAULT_TEXTURE_LENGTH: f32 = 2.0;
const DEFAULT_TEXTURE_SPEED: f32 = 1.0;
const DEFAULT_SEGMENTS: u32 = 10;

/// How `Beam.Texture`/`TextureMode` repeat along the curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextureMode {
    /// `TextureLength` repetitions across the whole beam.
    Stretch,
    /// One repetition every `TextureLength` studs.
    ///
    /// `Static` (ordinal 2) folds into this too: the two only differ once a
    /// beam's length changes between frames, which never happens in this
    /// viewer — no script or physics moves an `Attachment` — so treating them
    /// alike is exact here rather than an approximation, and is a documented
    /// v1 scope note, not a bug.
    Wrap,
}

/// A `Beam`'s static definition: its curve already in world space and every
/// property [`crate::renderer::beam`] needs to build its ribbon each frame.
#[derive(Clone)]
pub(crate) struct Beam {
    pub(crate) curve: Curve,
    pub(crate) width0: f32,
    pub(crate) width1: f32,
    pub(crate) color: ColorSequence,
    pub(crate) transparency: NumberSequence,
    /// [`AssetRef::Empty`] means "no texture": a `Beam` with none draws as a
    /// flat-coloured ribbon rather than falling back to a placeholder image
    /// the way a texture-less `ParticleEmitter` never can.
    pub(crate) texture: AssetRef,
    pub(crate) texture_length: f32,
    pub(crate) texture_mode: TextureMode,
    pub(crate) texture_speed: f32,
    pub(crate) light_emission: f32,
    pub(crate) face_camera: bool,
    /// World-space `Attachment0`/`Attachment1` **Y** axis (`SecondaryAxis`),
    /// used to orient the ribbon when `face_camera` is false — see the task
    /// brief's "plane given by the attachments' secondary axes".
    pub(crate) secondary_axis0: Vec3,
    pub(crate) secondary_axis1: Vec3,
    pub(crate) segments: u32,
    pub(crate) z_offset: f32,
}

/// Every enabled, placeable `Beam` in the DOM.
///
/// A `Beam` whose `Attachment0`/`Attachment1` cannot be resolved to a world
/// CFrame (dangling ref, missing `CFrame`, or a parent-less attachment) is
/// dropped outright, mirroring `Enabled = false` — Roblox itself simply does
/// not display such a beam.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase) -> Vec<Beam> {
    let parents = ParentMap::build(dom);
    descendants(dom)
        .filter(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| database.is_subclass_of(instance.class(), CLASS))
        })
        .filter_map(|referent| build(dom, &parents, dom.get(referent)?.properties()))
        .collect()
}

fn build(
    dom: &WeakDom,
    parents: &ParentMap<'_>,
    properties: &BTreeMap<String, Variant>,
) -> Option<Beam> {
    if !bool_or(properties, "Enabled", true) {
        return None;
    }

    let attachment0 = ref_or(properties, "Attachment0")?;
    let attachment1 = ref_or(properties, "Attachment1")?;
    let start = attachment::world_cframe(dom, parents, attachment0)?;
    let end = attachment::world_cframe(dom, parents, attachment1)?;

    let axis0 = start.x_axis.truncate().normalize_or_zero();
    let axis1 = end.x_axis.truncate().normalize_or_zero();
    let curve = Curve::new(
        start.w_axis.truncate(),
        axis0,
        float_or(properties, "CurveSize0", 0.0),
        end.w_axis.truncate(),
        axis1,
        float_or(properties, "CurveSize1", 0.0),
    );

    Some(Beam {
        curve,
        width0: float_or(properties, "Width0", DEFAULT_WIDTH).max(0.0),
        width1: float_or(properties, "Width1", DEFAULT_WIDTH).max(0.0),
        color: color_sequence_or(properties, "Color", [1.0, 1.0, 1.0]),
        transparency: number_sequence_or(properties, "Transparency", &[(0.0, 0.0)]),
        texture: texture_ref(properties),
        texture_length: float_or(properties, "TextureLength", DEFAULT_TEXTURE_LENGTH).max(1e-3),
        texture_mode: texture_mode_or(properties, "TextureMode", TextureMode::Stretch),
        texture_speed: float_or(properties, "TextureSpeed", DEFAULT_TEXTURE_SPEED),
        light_emission: float_or(properties, "LightEmission", 0.0).clamp(0.0, 1.0),
        face_camera: bool_or(properties, "FaceCamera", false),
        secondary_axis0: start.y_axis.truncate().normalize_or_zero(),
        secondary_axis1: end.y_axis.truncate().normalize_or_zero(),
        segments: int_or(properties, "Segments", DEFAULT_SEGMENTS).max(1),
        z_offset: float_or(properties, "ZOffset", 0.0),
    })
}

fn bool_or(properties: &BTreeMap<String, Variant>, key: &str, default: bool) -> bool {
    match properties.get(key) {
        Some(Variant::Bool(value)) => *value,
        _ => default,
    }
}

fn float_or(properties: &BTreeMap<String, Variant>, key: &str, default: f32) -> f32 {
    match properties.get(key) {
        Some(Variant::Float32(value)) if value.is_finite() => *value,
        Some(Variant::Float64(value)) if value.is_finite() => *value as f32,
        _ => default,
    }
}

fn int_or(properties: &BTreeMap<String, Variant>, key: &str, default: u32) -> u32 {
    match properties.get(key) {
        Some(&Variant::Int32(value)) if value > 0 => value as u32,
        Some(&Variant::Int64(value)) if value > 0 => value as u32,
        _ => default,
    }
}

fn ref_or(properties: &BTreeMap<String, Variant>, key: &str) -> Option<Ref> {
    match properties.get(key)? {
        Variant::Ref(referent) => Some(*referent),
        _ => None,
    }
}

fn number_sequence_or(
    properties: &BTreeMap<String, Variant>,
    key: &str,
    default: &[(f32, f32)],
) -> NumberSequence {
    match properties.get(key) {
        Some(Variant::NumberSequence(sequence)) => sequence.clone(),
        _ => NumberSequence {
            keypoints: default
                .iter()
                .map(|&(time, value)| NumberSequenceKeypoint {
                    time,
                    value,
                    envelope: 0.0,
                })
                .collect(),
        },
    }
}

fn color_sequence_or(
    properties: &BTreeMap<String, Variant>,
    key: &str,
    default: [f32; 3],
) -> ColorSequence {
    match properties.get(key) {
        Some(Variant::ColorSequence(sequence)) => sequence.clone(),
        _ => ColorSequence {
            keypoints: [0.0, 1.0]
                .into_iter()
                .map(|time| ColorSequenceKeypoint {
                    time,
                    color: rbx_dom::Color3Data {
                        r: default[0],
                        g: default[1],
                        b: default[2],
                    },
                    envelope: 0.0,
                })
                .collect(),
        },
    }
}

fn texture_mode_or(
    properties: &BTreeMap<String, Variant>,
    key: &str,
    default: TextureMode,
) -> TextureMode {
    match properties.get(key) {
        Some(&Variant::Enum(0)) => TextureMode::Stretch,
        Some(&Variant::Enum(1)) | Some(&Variant::Enum(2)) => TextureMode::Wrap,
        _ => default,
    }
}

/// `Texture` reaches us either as a plain string or wrapped in a `Content`;
/// absent, empty or unparsable all mean "no texture" (see [`Beam::texture`]).
fn texture_ref(properties: &BTreeMap<String, Variant>) -> AssetRef {
    properties
        .get("Texture")
        .and_then(asset_uri)
        .and_then(|uri| AssetRef::parse(uri).ok())
        .unwrap_or(AssetRef::Empty)
}

#[cfg(test)]
#[path = "instance/tests.rs"]
mod tests;
