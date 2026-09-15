//! Reads a `ParticleEmitter` instance and its parent `BasePart` into the
//! static definition [`sim`](super::sim) advances every frame.

use std::collections::{BTreeMap, HashMap};

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{
    ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint, Ref, Variant,
    WeakDom,
};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{self, Placement};
use crate::textures::{asset_uri, NormalId};

const CLASS: &str = "ParticleEmitter";
/// Roblox's own fallback image, used whenever `Texture` is absent or empty.
const DEFAULT_TEXTURE: &str = "rbxasset://textures/particles/sparkles_main.dds";
/// Per-emitter and whole-scene budgets from the task brief: a place with dozens
/// of continuously-emitting effects must stay a few tens of thousands of quads,
/// not spiral with however many emitters a map happens to have.
pub(crate) const PER_EMITTER_CAP: u32 = 2000;
pub(crate) const TOTAL_CAP: u32 = 20_000;

/// A `ParticleEmitter`'s static definition: everything [`super::sim::Simulation`]
/// needs to spawn and advance particles, already resolved into world space so
/// stepping it never has to touch the DOM again.
#[derive(Clone)]
pub(crate) struct Emitter {
    pub(crate) rate: f32,
    pub(crate) lifetime: (f32, f32),
    pub(crate) speed: (f32, f32),
    /// Degrees, matching the property's own unit; converted to radians at spawn
    /// time, once per particle rather than once per emitter.
    pub(crate) spread_degrees: (f32, f32),
    /// World-space, unit length: `EmissionDirection`'s local axis rotated by the
    /// parent part's own orientation.
    pub(crate) direction: Vec3,
    pub(crate) acceleration: Vec3,
    pub(crate) drag: f32,
    pub(crate) size: NumberSequence,
    pub(crate) transparency: NumberSequence,
    pub(crate) color: ColorSequence,
    pub(crate) texture: AssetRef,
    pub(crate) light_emission: f32,
    pub(crate) rotation_degrees: (f32, f32),
    pub(crate) rot_speed_degrees: (f32, f32),
    pub(crate) z_offset: f32,
    /// How many live particles this emitter may hold at once, already folded
    /// against [`PER_EMITTER_CAP`] and whatever [`TOTAL_CAP`] had left over.
    pub(crate) cap: u32,
    /// Seeds this emitter's own [`super::rng::Rng`], derived from its referent so
    /// two runs over the same file always draw the same particles.
    pub(crate) seed: u64,
    /// The `ParticleEmitter` instance this was read from, which is how a
    /// re-planned list (see `Scene::replan_effect`) is matched back to the
    /// renderer's running simulations — `seed` alone is not reversible.
    pub(crate) referent: Ref,
    /// The parent part's placement, unit-cube-to-world: sampling a spawn point
    /// only needs `volume.transform_point3` on a `[-0.5, 0.5]^3` local point.
    pub(crate) volume: Mat4,
}

/// Walks `dom` for every `ParticleEmitter` parented to a drawn `BasePart`,
/// spending [`TOTAL_CAP`] across them in the order the DOM lists them.
///
/// Workspace-scoped, same as `Scene::from_dom`'s own part build: an emitter
/// staged outside `Workspace` never draws (its `placements` lookup below would
/// already reject it, since `placements` is itself Workspace-scoped, but
/// searching only `Workspace` to begin with saves walking the rest of a large
/// place for nothing).
///
/// An `Attachment` parent is skipped (not in `placements`, which only carries
/// `BasePart`s) — a documented v1 gap, not a bug: see the task's TODO on
/// Attachment parents. Likewise `Enabled = false` is skipped outright, since a
/// disabled emitter never spawns anything in this viewer (no script runs to
/// flip it back on) and downloading its texture would be wasted network.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
) -> Vec<Emitter> {
    let parents = parent_map(dom, database);
    let mut budget = TOTAL_CAP;

    scene::workspace_descendants(dom, database)
        .filter(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| database.is_subclass_of(instance.class(), CLASS))
        })
        .filter_map(|referent| {
            let instance = dom.get(referent)?;
            let parent = *parents.get(&referent)?;
            let volume = placements.get(&parent)?.model;
            build(instance.properties(), volume, referent, &mut budget)
        })
        .collect()
}

/// Maps every instance to the referent of its own parent, since [`rbx_dom`]'s
/// `Instance` keeps children lists but no back-pointer.
///
/// Workspace-scoped: an emitter this module ever looks up a parent for was
/// itself just found under `Workspace`, so its own parent chain never leaves
/// `Workspace` either.
fn parent_map(dom: &WeakDom, database: &ReflectionDatabase) -> HashMap<Ref, Ref> {
    let mut parents = HashMap::new();
    for referent in scene::workspace_descendants(dom, database) {
        if let Some(instance) = dom.get(referent) {
            for &child in instance.children() {
                parents.insert(child, referent);
            }
        }
    }
    parents
}

fn build(
    properties: &BTreeMap<String, Variant>,
    volume: Mat4,
    referent: Ref,
    budget: &mut u32,
) -> Option<Emitter> {
    if !bool_or(properties, "Enabled", true) {
        return None;
    }

    let rate = float_or(properties, "Rate", 20.0).max(0.0);
    let lifetime = number_range_or(properties, "Lifetime", (5.0, 10.0));
    let direction_id = normal_id_or(properties, "EmissionDirection", NormalId::Top);

    let uncapped = (rate * lifetime.1.max(0.0)).ceil().max(0.0) as u32;
    let cap = uncapped.min(PER_EMITTER_CAP).min(*budget);
    *budget = budget.saturating_sub(cap);

    Some(Emitter {
        rate,
        lifetime,
        speed: number_range_or(properties, "Speed", (5.0, 5.0)),
        spread_degrees: vector2_or(properties, "SpreadAngle", (0.0, 0.0)),
        direction: world_axis(volume, direction_id.axis()),
        acceleration: vector3_or(properties, "Acceleration", Vec3::ZERO),
        drag: float_or(properties, "Drag", 0.0),
        size: number_sequence_or(properties, "Size", &[(0.0, 1.0), (1.0, 1.0)]),
        transparency: number_sequence_or(properties, "Transparency", &[(0.0, 0.0), (1.0, 0.0)]),
        color: color_sequence_or(properties, "Color", [1.0, 1.0, 1.0]),
        texture: texture_ref(properties),
        light_emission: float_or(properties, "LightEmission", 0.0).clamp(0.0, 1.0),
        rotation_degrees: number_range_or(properties, "Rotation", (0.0, 0.0)),
        rot_speed_degrees: number_range_or(properties, "RotSpeed", (0.0, 0.0)),
        z_offset: float_or(properties, "ZOffset", 0.0),
        cap,
        seed: seed_of(referent.value()),
        referent,
        volume,
    })
}

/// Rotates a part-local unit axis into world space using `model`'s own basis
/// columns, normalized so a non-uniformly sized part (a `Box.Size` with three
/// different studs) still yields a unit direction instead of a skewed one.
fn world_axis(model: Mat4, local: Vec3) -> Vec3 {
    let x = model.x_axis.truncate().normalize_or_zero();
    let y = model.y_axis.truncate().normalize_or_zero();
    let z = model.z_axis.truncate().normalize_or_zero();
    (x * local.x + y * local.y + z * local.z).normalize_or_zero()
}

/// Murmur3-style finalizer: turns a small, densely-packed referent id into a
/// well-mixed 64-bit seed, so neighbouring emitters (adjacent ids) do not draw
/// visibly correlated particle streams.
fn seed_of(referent: u32) -> u64 {
    let mut x = u64::from(referent) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    x
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

fn number_range_or(
    properties: &BTreeMap<String, Variant>,
    key: &str,
    default: (f32, f32),
) -> (f32, f32) {
    match properties.get(key) {
        Some(Variant::NumberRange(range)) => (range.min, range.max),
        _ => default,
    }
}

fn vector2_or(
    properties: &BTreeMap<String, Variant>,
    key: &str,
    default: (f32, f32),
) -> (f32, f32) {
    match properties.get(key) {
        Some(Variant::Vector2(v)) => (v.x, v.y),
        _ => default,
    }
}

fn vector3_or(properties: &BTreeMap<String, Variant>, key: &str, default: Vec3) -> Vec3 {
    match properties.get(key) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => default,
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

fn normal_id_or(properties: &BTreeMap<String, Variant>, key: &str, default: NormalId) -> NormalId {
    match properties.get(key) {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw).unwrap_or(default),
        _ => default,
    }
}

/// `Texture` (a `Content` or, in older files, a plain string), falling back to
/// Roblox's own default image when absent, empty, or unparsable.
fn texture_ref(properties: &BTreeMap<String, Variant>) -> AssetRef {
    properties
        .get("Texture")
        .and_then(asset_uri)
        .and_then(|uri| AssetRef::parse(uri).ok())
        .filter(|reference| *reference != AssetRef::Empty)
        .unwrap_or_else(|| AssetRef::parse(DEFAULT_TEXTURE).expect("default texture URI is valid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(entries: Vec<(&str, Variant)>) -> BTreeMap<String, Variant> {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    #[test]
    fn a_disabled_emitter_builds_nothing() {
        let mut budget = TOTAL_CAP;
        let properties = props(vec![("Enabled", Variant::Bool(false))]);
        assert!(build(&properties, Mat4::IDENTITY, Ref::new(1), &mut budget).is_none());
        assert_eq!(budget, TOTAL_CAP, "a skipped emitter spends no budget");
    }

    #[test]
    fn an_emitter_with_no_properties_falls_back_to_documented_defaults() {
        let mut budget = TOTAL_CAP;
        let emitter = build(&props(vec![]), Mat4::IDENTITY, Ref::new(1), &mut budget).unwrap();
        assert_eq!(emitter.rate, 20.0);
        assert_eq!(emitter.lifetime, (5.0, 10.0));
        assert_eq!(emitter.texture, AssetRef::parse(DEFAULT_TEXTURE).unwrap());
        assert_eq!(
            emitter.direction,
            Vec3::Y,
            "default EmissionDirection is Top"
        );
    }

    #[test]
    fn cap_is_rate_times_max_lifetime_clamped_to_the_per_emitter_ceiling() {
        let mut budget = TOTAL_CAP;
        let properties = props(vec![
            ("Rate", Variant::Float32(10.0)),
            (
                "Lifetime",
                Variant::NumberRange(rbx_dom::NumberRange { min: 1.0, max: 3.0 }),
            ),
        ]);
        let emitter = build(&properties, Mat4::IDENTITY, Ref::new(1), &mut budget).unwrap();
        assert_eq!(emitter.cap, 30);

        let huge = props(vec![
            ("Rate", Variant::Float32(1_000_000.0)),
            (
                "Lifetime",
                Variant::NumberRange(rbx_dom::NumberRange { min: 1.0, max: 1.0 }),
            ),
        ]);
        let mut budget = TOTAL_CAP;
        let emitter = build(&huge, Mat4::IDENTITY, Ref::new(2), &mut budget).unwrap();
        assert_eq!(emitter.cap, PER_EMITTER_CAP);
    }

    #[test]
    fn the_whole_scene_budget_is_shared_across_emitters_in_order() {
        let properties = props(vec![
            ("Rate", Variant::Float32(1_000_000.0)),
            (
                "Lifetime",
                Variant::NumberRange(rbx_dom::NumberRange { min: 1.0, max: 1.0 }),
            ),
        ]);
        let mut budget = PER_EMITTER_CAP + 500;
        let first = build(&properties, Mat4::IDENTITY, Ref::new(1), &mut budget).unwrap();
        let second = build(&properties, Mat4::IDENTITY, Ref::new(2), &mut budget).unwrap();
        assert_eq!(first.cap, PER_EMITTER_CAP);
        assert_eq!(second.cap, 500);
    }

    #[test]
    fn world_axis_rotates_a_local_axis_by_the_part_orientation() {
        // A part rotated 90 degrees around Z: local +Y now points along -X.
        let rotation = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let axis = world_axis(rotation, Vec3::Y);
        assert!((axis - Vec3::NEG_X).length() < 1e-5);
    }

    #[test]
    fn two_different_referents_seed_different_streams() {
        assert_ne!(seed_of(1), seed_of(2));
    }
}
