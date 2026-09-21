//! Reads a `ParticleEmitter` instance and the `BasePart` or `Attachment` it
//! hangs off into the static definition [`sim`](super::sim) advances every
//! frame, and walks a DOM for every emitter a place holds — the
//! preconfigured `Fire`/`Smoke`/`Sparkles` classes included, which
//! [`legacy`](super::legacy) reads into emitters of this same shape.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{ColorSequence, NumberSequence, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::beam::{world_cframe, ParentMap};
use crate::scene::{self, Placement};
use crate::textures::{asset_uri, NormalId};

use super::legacy;
use crate::scene::props::{
    bool_or, color_sequence_or, float_or, normal_id_or, number_range_or, number_sequence_or,
    vector2_or, vector3_or, Properties,
};

const CLASS: &str = "ParticleEmitter";
/// Roblox's own fallback image, used whenever `Texture` is absent or empty.
const DEFAULT_TEXTURE: &str = "rbxasset://textures/particles/sparkles_main.dds";
/// Per-emitter and whole-scene budgets from the task brief: a place with dozens
/// of continuously-emitting effects must stay a few tens of thousands of quads,
/// not spiral with however many emitters a map happens to have.
pub(crate) const PER_EMITTER_CAP: u32 = 2000;
pub(crate) const TOTAL_CAP: u32 = 20_000;

/// Where an emitter spawns and which way it points.
///
/// Two matrices rather than one: a `BasePart` parent emits from anywhere
/// inside its own volume, while an `Attachment` parent — documented on every
/// one of these classes as the way to move the emission position and
/// direction off the part's centre — is a single point with a frame of its
/// own and no volume at all.
#[derive(Clone, Copy)]
pub(crate) struct Origin {
    /// Unit-cube-to-world: a `[-0.5, 0.5]^3` local point through this is a
    /// spawn position.
    volume: Mat4,
    /// The frame `EmissionDirection`'s local axis is rotated by.
    frame: Mat4,
}

impl Origin {
    /// A `BasePart` parent: the whole part is the spawn volume.
    fn of_part(model: Mat4) -> Self {
        Origin {
            volume: model,
            frame: model,
        }
    }

    /// An `Attachment` parent: every particle starts at the one point, so the
    /// volume collapses to the attachment's origin while its orientation
    /// still aims them.
    fn of_attachment(cframe: Mat4) -> Self {
        Origin {
            volume: cframe * Mat4::from_scale(Vec3::ZERO),
            frame: cframe,
        }
    }

    /// The emitter's own up: where a `Fire`'s flames rise and a `Smoke`'s
    /// plume drifts, both documented as the parent's **+Y**.
    pub(super) fn up(&self) -> Vec3 {
        world_axis(self.frame, Vec3::Y)
    }

    /// Where a preconfigured effect spawns. `Fire`, `Smoke` and `Sparkles`
    /// all document emission from the *centre* of the parent part rather
    /// than from anywhere inside it, so the volume collapses to that point —
    /// or, for an `Attachment` parent, to the attachment's own.
    pub(super) fn centre(&self) -> Mat4 {
        self.frame * Mat4::from_scale(Vec3::ZERO)
    }
}

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
    /// `TimeScale`, clamped to the `[0, 1]` its docs give: the whole effect
    /// runs that much of normal speed, and `0` freezes it. Applied to the
    /// step the renderer advances the simulation by rather than to any one
    /// property, which is what makes it "the speed of the effect" rather
    /// than a slower spawn rate.
    pub(crate) time_scale: f32,
    /// How many live particles this emitter may hold at once, already folded
    /// against [`PER_EMITTER_CAP`] and whatever [`TOTAL_CAP`] had left over.
    pub(crate) cap: u32,
    /// Seeds this emitter's own [`super::rng::Rng`], derived from its referent so
    /// two runs over the same file always draw the same particles.
    pub(crate) seed: u64,
    /// The instance this was read from, which is how a re-planned list (see
    /// `Scene::replan_effect`) is matched back to the renderer's running
    /// simulations — `seed` alone is not reversible.
    pub(crate) referent: Ref,
    /// Which of that instance's emitters this is: a `ParticleEmitter` is one,
    /// a `Fire` is documented as two. Part of the identity a re-plan matches
    /// on, so a `Fire`'s inner flame never inherits its outer flame's
    /// particles.
    pub(crate) slot: u8,
    /// The parent's placement, unit-cube-to-world: sampling a spawn point
    /// only needs `volume.transform_point3` on a `[-0.5, 0.5]^3` local point.
    pub(crate) volume: Mat4,
}

impl Emitter {
    /// What a re-plan matches a running simulation by — see [`Emitter::slot`].
    pub(crate) fn id(&self) -> (Ref, u8) {
        (self.referent, self.slot)
    }
}

/// Walks `dom` for every emitting instance parented to a drawn `BasePart` or
/// to an `Attachment` inside one, spending [`TOTAL_CAP`] across them in the
/// order the DOM lists them.
///
/// Workspace-scoped, same as `Scene::from_dom`'s own part build: an emitter
/// staged outside `Workspace` never draws, and searching only `Workspace`
/// saves walking the rest of a large place for nothing.
///
/// `Enabled = false` is skipped outright, since a disabled emitter never
/// spawns anything in this viewer (no script runs to flip it back on) and
/// downloading its texture would be wasted network.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
) -> Vec<Emitter> {
    let parents = ParentMap::build(dom);
    let mut budget = TOTAL_CAP;
    let mut emitters = Vec::new();

    for referent in scene::workspace_descendants(dom, database) {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        let class = instance.class();
        let particle_emitter = database.is_subclass_of(class, CLASS);
        if !particle_emitter && !legacy::is_legacy(class) {
            continue;
        }
        let Some(origin) = origin_of(dom, &parents, placements, referent) else {
            continue;
        };
        let properties = instance.properties();
        if particle_emitter {
            emitters.extend(build(properties, origin, referent, &mut budget));
        } else {
            legacy::build(
                class,
                properties,
                origin,
                referent,
                &mut budget,
                &mut emitters,
            );
        }
    }
    emitters
}

/// Where the instance at `referent` emits from: its parent part's volume, or
/// the point of the `Attachment` it hangs off. `None` for a parent that is
/// neither — a `Folder`, or a part outside the Workspace, neither of which
/// this viewer draws anything for.
fn origin_of(
    dom: &WeakDom,
    parents: &ParentMap<'_>,
    placements: &HashMap<Ref, Placement>,
    referent: Ref,
) -> Option<Origin> {
    let parent = dom.parent(referent)?;
    match placements.get(&parent) {
        Some(placement) => Some(Origin::of_part(placement.model)),
        None => world_cframe(dom, parents, parent).map(Origin::of_attachment),
    }
}

fn build(
    properties: &Properties,
    origin: Origin,
    referent: Ref,
    budget: &mut u32,
) -> Option<Emitter> {
    if !bool_or(properties, "Enabled", true) {
        return None;
    }

    let rate = float_or(properties, "Rate", 20.0).max(0.0);
    let lifetime = number_range_or(properties, "Lifetime", (5.0, 10.0));
    let direction_id = normal_id_or(properties, "EmissionDirection", NormalId::Top);

    Some(Emitter {
        rate,
        lifetime,
        speed: number_range_or(properties, "Speed", (5.0, 5.0)),
        spread_degrees: vector2_or(properties, "SpreadAngle", (0.0, 0.0)),
        direction: world_axis(origin.frame, direction_id.axis()),
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
        time_scale: time_scale(properties),
        cap: spend(rate, lifetime.1, budget),
        seed: seed_of(referent.value(), 0),
        referent,
        slot: 0,
        volume: origin.volume,
    })
}

/// `TimeScale` as every emitting class documents it: between 0 and 1, `1`
/// normal speed, `0` frozen.
pub(super) fn time_scale(properties: &Properties) -> f32 {
    float_or(properties, "TimeScale", 1.0).clamp(0.0, 1.0)
}

/// Takes this emitter's share of the whole-scene particle budget: as many as
/// its own rate can keep alive, capped per emitter and by whatever is left.
pub(super) fn spend(rate: f32, max_lifetime: f32, budget: &mut u32) -> u32 {
    let uncapped = (rate * max_lifetime.max(0.0)).ceil().max(0.0) as u32;
    let cap = uncapped.min(PER_EMITTER_CAP).min(*budget);
    *budget = budget.saturating_sub(cap);
    cap
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
/// visibly correlated particle streams. `slot` mixes in as well, so a `Fire`'s
/// two flames are not the same stream drawn twice.
pub(super) fn seed_of(referent: u32, slot: u8) -> u64 {
    let mut x = (u64::from(referent) | (u64::from(slot) << 32)) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    x
}

/// `Texture` (a `Content` or, in older files, a plain string), falling back to
/// Roblox's own default image when absent, empty, or unparsable.
fn texture_ref(properties: &Properties) -> AssetRef {
    properties
        .get("Texture")
        .and_then(asset_uri)
        .and_then(|uri| AssetRef::parse(uri).ok())
        .filter(|reference| *reference != AssetRef::Empty)
        .unwrap_or_else(|| AssetRef::parse(DEFAULT_TEXTURE).expect("default texture URI is valid"))
}

#[cfg(test)]
#[path = "emitter/tests.rs"]
mod tests;
