//! Reads a `Trail` instance and resolves its two `Attachment` endpoints into
//! world *positions* — the identical walk `scene::beam::attachment` does,
//! reused directly through `scene::beam`'s re-export rather than duplicated,
//! since a `Trail` follows the exact same `Attachment0`/`Attachment1` Refs a
//! `Beam` does.

use std::collections::BTreeMap;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{
    ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref, Variant,
    WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use crate::scene::beam::{self, ParentMap};
use crate::scene::workspace_descendants;
use crate::textures::asset_uri;

const CLASS: &str = "Trail";

/// Studio's own defaults, so a `Trail` that omits a property lands where the
/// property sheet shows it — the same reasoning `scene::beam::instance`'s own
/// constants document.
const DEFAULT_LIFETIME: f32 = 2.0;
const DEFAULT_MIN_LENGTH: f32 = 0.1;
const DEFAULT_TEXTURE_LENGTH: f32 = 1.0;

/// A `Trail`'s static definition: both attachments' world position, resolved
/// once at scene-build time, and every appearance property
/// [`super::ribbon`]/`renderer::trail` need. `renderer::trail` feeds
/// `position0`/`position1` to a [`super::Recorder`] every frame — in this
/// viewer that is the same pair forever, which is exactly why nothing is ever
/// recorded past the first sample (see `scene::trail`'s module doc).
///
/// `FaceCamera` is not a field: this viewer always draws a trail facing the
/// camera (see `renderer::trail::ribbon`'s doc), a deliberate v1 scope note
/// rather than an oversight — the same way `scene::beam::instance::TextureMode`
/// folds `Wrap`/`Static` together. `TextureMode` itself is out of scope for
/// the same reason: `texture_length` is always "studs per repeat" here (see
/// the task brief), i.e. `Beam`'s `Wrap` behaviour unconditionally.
#[derive(Clone)]
pub(crate) struct Trail {
    pub(crate) position0: Vec3,
    pub(crate) position1: Vec3,
    /// `false` does not hide a trail that already has history — only new
    /// segments stop being recorded (see `Trail.Enabled`'s docs) — so
    /// `renderer::trail` reads this to gate `Recorder::record`, never to drop
    /// the trail outright the way `scene::beam` drops a disabled `Beam`.
    pub(crate) enabled: bool,
    pub(crate) lifetime: f32,
    pub(crate) min_length: f32,
    pub(crate) width_scale: NumberSequence,
    pub(crate) color: ColorSequence,
    pub(crate) transparency: NumberSequence,
    /// [`AssetRef::Empty`] means "no texture": a `Trail` with none draws as a
    /// flat-coloured ribbon, the same fallback `Beam.Texture` gets.
    pub(crate) texture: AssetRef,
    pub(crate) texture_length: f32,
    pub(crate) light_emission: f32,
}

/// Every placeable `Trail` in the DOM: both `Attachment0`/`Attachment1`
/// resolved to a world position.
///
/// A `Trail` that cannot be placed (dangling ref, missing `CFrame`, or a
/// parent-less attachment) is dropped outright: Roblox itself has nothing to
/// draw a trail between in that case either.
///
/// The `Trail` search itself is Workspace-scoped, same as `Scene::from_dom`'s
/// own part build: real Studio never draws a trail outside `Workspace`. Its
/// endpoints, resolved through `ParentMap`, only ever walk *up* from a
/// `Trail` already found there, so they stay inside `Workspace` for free.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase) -> Vec<Trail> {
    let parents = ParentMap::build(dom);
    workspace_descendants(dom, database)
        .filter(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| database.is_subclass_of(instance.class(), CLASS))
        })
        .filter_map(|referent| build(dom, &parents, dom.get(referent)?.properties()))
        .collect()
}

fn build(
    dom: &WeakDom,
    parents: &ParentMap,
    properties: &BTreeMap<String, Variant>,
) -> Option<Trail> {
    let attachment0 = ref_or(properties, "Attachment0")?;
    let attachment1 = ref_or(properties, "Attachment1")?;
    let position0 = beam::world_cframe(dom, parents, attachment0)?
        .w_axis
        .truncate();
    let position1 = beam::world_cframe(dom, parents, attachment1)?
        .w_axis
        .truncate();

    Some(Trail {
        position0,
        position1,
        enabled: bool_or(properties, "Enabled", true),
        lifetime: float_or(properties, "Lifetime", DEFAULT_LIFETIME).max(1e-3),
        min_length: float_or(properties, "MinLength", DEFAULT_MIN_LENGTH).max(0.0),
        width_scale: number_sequence_or(properties, "WidthScale", &[(0.0, 1.0), (1.0, 1.0)]),
        color: color_sequence_or(properties, "Color", [1.0, 1.0, 1.0]),
        transparency: number_sequence_or(properties, "Transparency", &[(0.0, 0.0)]),
        texture: texture_ref(properties),
        texture_length: float_or(properties, "TextureLength", DEFAULT_TEXTURE_LENGTH).max(1e-3),
        light_emission: float_or(properties, "LightEmission", 0.0).clamp(0.0, 1.0),
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

/// `Texture` reaches us either as a plain string or wrapped in a `Content`;
/// absent, empty or unparsable all mean "no texture" (see [`Trail::texture`]).
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
