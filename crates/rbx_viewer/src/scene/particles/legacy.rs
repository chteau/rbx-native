//! The three preconfigured particle classes — `Fire`, `Smoke` and `Sparkles`
//! — read into the same [`Emitter`] definition a `ParticleEmitter` produces,
//! so everything downstream (the budget, the simulation, the billboards, the
//! Properties-panel re-plan) treats them identically.
//!
//! Each is documented as "a preconfigured particle emitter": the class
//! exposes a handful of knobs and the engine supplies the rest. Roblox
//! publishes the knobs and the *shape* of each effect — a `Fire` is two
//! emitters, its inner one longer-lived, faster-rising and at
//! `LightEmission` 1; a `Smoke`'s particles are "more than twice" their
//! `Size` in studs; a `Sparkles`' colour "very faintly animate[s] between a
//! subtle green and red" — but not the underlying emitter parameters. What
//! the docs state is applied here as stated and marked as such; every other
//! number is this renderer's own, picked to read as the effect at its
//! documented ranges, and marked as that instead.
//!
//! `Explosion` is the fourth class of this family and is deliberately not
//! among them: it is a one-shot that plays when it is parented into the
//! world and destroys itself afterwards (its `BlastRadius` and
//! `BlastPressure` are a physics impulse, not a standing effect), so an
//! `Explosion` sitting in a place file has nothing to draw — the same
//! reading this project already takes of `VideoFrame` playback.

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::emitter::{seed_of, spend, time_scale, Emitter, Origin};
use crate::scene::props::{
    bool_or, color3_of_any, color_sequence, flat_color, float_of_any, number_sequence, Properties,
};

const FIRE: &str = "Fire";
const SMOKE: &str = "Smoke";
const SPARKLES: &str = "Sparkles";

/// Roblox's own images for these three effects, from the Studio content
/// package that already backs every other `rbxasset://` texture here. Which
/// file the engine itself draws each class with is not published; these are
/// the package's own, named for the effect they belong to.
const FIRE_TEXTURE: &str = "rbxasset://textures/particles/fire_main.dds";
const SMOKE_TEXTURE: &str = "rbxasset://textures/particles/smoke_main.dds";
const SPARKLES_TEXTURE: &str = "rbxasset://textures/particles/sparkles_main.dds";

/// `Fire.Heat` and `Smoke.RiseVelocity` are both documented as limited to
/// [-25, 25]; the engine clamps, so a file carrying more does not emit faster
/// than one carrying the limit.
const MAX_RISE: f32 = 25.0;
/// `Fire.Size`'s documented range.
const FIRE_SIZE_RANGE: (f32, f32) = (2.0, 30.0);
/// `Smoke.Size`'s documented range.
const SMOKE_SIZE_RANGE: (f32, f32) = (0.1, 100.0);

/// Studs per unit of `Fire.Size`, for the outer flame: the docs say only that
/// the flames come out "somewhat smaller" than the size in studs, so the
/// figure itself is this renderer's.
const FIRE_STUDS: f32 = 0.6;
/// The inner flame, which the docs describe as the "smaller, secondary"
/// emitter — how much smaller is not published.
const FIRE_INNER_SCALE: f32 = 0.55;
/// Studs per unit of `Smoke.Size`. The docs pin this one down twice over:
/// "more than twice as large" as the size in studs, and "at the largest size"
/// — 100 — "smoke particles can render larger than 200 studs wide".
const SMOKE_STUDS: f32 = 2.2;
/// How much brighter than `Color` a flame draws — see [`Emitter::gain`],
/// which explains why one is needed at all. Chosen so a default `Fire` over
/// Roblox's own dark flame image reads as a flame; the inner one is brighter
/// still, being the hotter of the two.
const FIRE_GAIN: (f32, f32) = (5.0, 7.0);

/// How wide a sparkle is drawn. Nothing in `Sparkles` sizes its particles at
/// all, so this is a fixed figure of this renderer's own.
const SPARKLE_STUDS: f32 = 1.5;

/// Whether a class is one of the three this module reads. An exact match
/// rather than a subclass test: all three are documented as `Instance`
/// subclasses with no descendants of their own.
pub(super) fn is_legacy(class: &str) -> bool {
    matches!(class, FIRE | SMOKE | SPARKLES)
}

/// Appends whatever emitters `class` is documented as being — two for a
/// `Fire`, one each for a `Smoke` and a `Sparkles`, none for any of them
/// while `Enabled` is false — to `into`.
pub(super) fn build(
    class: &str,
    properties: &Properties,
    origin: Origin,
    referent: Ref,
    budget: &mut u32,
    into: &mut Vec<Emitter>,
) {
    if !bool_or(properties, "Enabled", true) {
        return;
    }
    match class {
        FIRE => {
            into.push(fire(properties, origin, referent, budget, Flame::Outer));
            into.push(fire(properties, origin, referent, budget, Flame::Inner));
        }
        SMOKE => into.push(smoke(properties, origin, referent, budget)),
        SPARKLES => into.push(sparkles(properties, origin, referent, budget)),
        _ => {}
    }
}

/// Which of a `Fire`'s two documented emitters is being built: the primary
/// (outer) one, or the smaller secondary (inner) one whose particles have "a
/// significantly longer lifetime (and rise farther)".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flame {
    Outer,
    Inner,
}

fn fire(
    properties: &Properties,
    origin: Origin,
    referent: Ref,
    budget: &mut u32,
    flame: Flame,
) -> Emitter {
    // Every spelling a file may hold them under, the one Roblox saves first
    // (`SerializesAs` in `assets/reflection-defaults.json`): a real `Fire`
    // stores `size_xml` and `heat_xml`, never `Size` or `Heat`, and an edit
    // to one keeps writing where the value already is.
    let size = float_of_any(properties, &["size_xml", "Size", "size"], 5.0)
        .clamp(FIRE_SIZE_RANGE.0, FIRE_SIZE_RANGE.1);
    let heat = float_of_any(properties, &["heat_xml", "Heat"], 9.0).clamp(-MAX_RISE, MAX_RISE);
    // Positive `Heat` is up, negative is down — documented — and how fast is
    // "the velocity at which particles are emit", with no studs-per-second
    // conversion published. This renderer reads it as a fraction of a stud
    // per unit: enough that a default `Heat` lifts the flame clear of the
    // part it is emitted from the centre of, without shooting off it.
    let rise = origin.up() * heat.signum();
    let speed = heat.abs() * 0.35;

    let inner = flame == Flame::Inner;
    let studs = size * FIRE_STUDS * if inner { FIRE_INNER_SCALE } else { 1.0 };
    let lifetime = if inner { (1.2, 1.8) } else { (0.6, 0.9) };
    let rate = if inner { 16.0 } else { 26.0 };

    Emitter {
        rate,
        lifetime,
        // The inner flame is the one that "rise[s] farther", so it leaves
        // faster as well as living longer.
        speed: if inner {
            (speed * 1.1, speed * 1.6)
        } else {
            (speed * 0.7, speed * 1.1)
        },
        spread_degrees: if inner { (8.0, 8.0) } else { (14.0, 14.0) },
        direction: rise,
        // Documented: `Heat` "also affects the `ParticleEmitter.Acceleration`
        // of the inner particles". The outer ones are left to coast.
        acceleration: if inner {
            rise * heat.abs() * 0.25
        } else {
            Vec3::ZERO
        },
        drag: 0.0,
        // A flame narrows as it burns out.
        size: number_sequence(&[(0.0, studs * 0.55), (0.35, studs), (1.0, studs * 0.3)]),
        transparency: number_sequence(&[(0.0, 0.0), (0.6, 0.3), (1.0, 1.0)]),
        color: flat_color(color3_of_any(
            properties,
            &[if inner { "SecondaryColor" } else { "Color" }],
            if inner {
                [1.0, 0.0, 0.0]
            } else {
                [1.0, 0.68, 0.0]
            },
        )),
        texture: texture(FIRE_TEXTURE),
        // Documented for the inner particles: "the inner particles use a
        // `ParticleEmitter.LightEmission` of 1, so darker colors will instead
        // cause the particles to appear transparent". The outer flame's is
        // not published; it is given some, so a flame still glows.
        light_emission: if inner { 1.0 } else { 0.5 },
        rotation_degrees: (-180.0, 180.0),
        rot_speed_degrees: (-25.0, 25.0),
        z_offset: 0.0,
        gain: if inner { FIRE_GAIN.1 } else { FIRE_GAIN.0 },
        time_scale: time_scale(properties),
        cap: spend(rate, lifetime.1, budget),
        seed: seed_of(referent.value(), inner as u8),
        referent,
        slot: inner as u8,
        volume: origin.centre(),
    }
}

fn smoke(properties: &Properties, origin: Origin, referent: Ref, budget: &mut u32) -> Emitter {
    // Stored spellings first, the same as `fire`'s.
    let size = float_of_any(properties, &["size_xml", "Size"], 1.0)
        .clamp(SMOKE_SIZE_RANGE.0, SMOKE_SIZE_RANGE.1);
    let studs = size * SMOKE_STUDS;
    // Documented as behaving like `ParticleEmitter.Speed`, so it is read as
    // studs per second directly; negative emits downward.
    let rise = float_of_any(properties, &["riseVelocity_xml", "RiseVelocity"], 1.0)
        .clamp(-MAX_RISE, MAX_RISE);
    // Documented as the inverse of `Transparency`: 0 invisible, 1 visible.
    // The plume still thins out as it disperses, which is this renderer's.
    let opacity = float_of_any(properties, &["opacity_xml", "Opacity"], 0.5).clamp(0.0, 1.0);
    let rate = 14.0;
    let lifetime = (2.5, 4.0);

    Emitter {
        rate,
        lifetime,
        speed: (rise.abs() * 0.8, rise.abs() * 1.2),
        spread_degrees: (10.0, 10.0),
        direction: origin.up() * if rise < 0.0 { -1.0 } else { 1.0 },
        acceleration: Vec3::ZERO,
        drag: 0.0,
        // Smoke spreads as it rises.
        size: number_sequence(&[(0.0, studs * 0.7), (1.0, studs * 1.6)]),
        transparency: number_sequence(&[
            (0.0, 1.0 - opacity),
            (0.7, 1.0 - opacity * 0.6),
            (1.0, 1.0),
        ]),
        color: flat_color(color3_of_any(properties, &["Color"], [0.7, 0.7, 0.7])),
        texture: texture(SMOKE_TEXTURE),
        light_emission: 0.0,
        rotation_degrees: (-180.0, 180.0),
        rot_speed_degrees: (-12.0, 12.0),
        z_offset: 0.0,
        // Roblox's own smoke image is already white, so its colour needs no
        // help to come through.
        gain: 1.0,
        time_scale: time_scale(properties),
        cap: spend(rate, lifetime.1, budget),
        seed: seed_of(referent.value(), 0),
        referent,
        slot: 0,
        volume: origin.centre(),
    }
}

fn sparkles(properties: &Properties, origin: Origin, referent: Ref, budget: &mut u32) -> Emitter {
    // `SparkleColor` is the live name; `Color` is its deprecated twin, which
    // the docs say "functions identically", and older files carry that one.
    let tint = color3_of_any(properties, &["SparkleColor", "Color"], [1.0, 1.0, 1.0]);
    let rate = 40.0;
    // Long enough, and quick enough, to carry a sparkle clear of the part
    // it spawns in the middle of — every one of these classes emits from
    // the centre, so an effect that travels less than half a part never
    // shows at all.
    let lifetime = (1.0, 1.6);

    Emitter {
        rate,
        lifetime,
        speed: (2.0, 4.0),
        // Sparkles surround the object rather than streaming off one face.
        spread_degrees: (180.0, 180.0),
        direction: origin.up(),
        acceleration: Vec3::ZERO,
        drag: 0.0,
        size: number_sequence(&[
            (0.0, 0.0),
            (0.3, SPARKLE_STUDS),
            (0.7, SPARKLE_STUDS),
            (1.0, 0.0),
        ]),
        transparency: number_sequence(&[(0.0, 0.0), (1.0, 0.0)]),
        color: twinkle(tint),
        texture: texture(SPARKLES_TEXTURE),
        // Documented: "sparkles have a partial `ParticleEmitter.LightEmission`
        // effect, so dark colors tend to render more transparent and white
        // colors look very bright". How partial is not published.
        light_emission: 0.5,
        rotation_degrees: (-180.0, 180.0),
        rot_speed_degrees: (-60.0, 60.0),
        z_offset: 0.0,
        gain: 1.0,
        time_scale: time_scale(properties),
        cap: spend(rate, lifetime.1, budget),
        seed: seed_of(referent.value(), 0),
        referent,
        slot: 0,
        volume: origin.centre(),
    }
}

/// The documented "natural color sequence" a `Sparkles` applies on top of its
/// own colour: "sparkles very faintly animate between a subtle green and
/// red". Faint is the whole of what is published, so the shift is a small
/// one — the two off-channels dip rather than the named one rising, which
/// keeps a white `SparkleColor` from clipping.
fn twinkle(tint: [f32; 3]) -> rbx_dom::ColorSequence {
    const DIP: f32 = 0.85;
    color_sequence(&[
        (0.0, [tint[0] * DIP, tint[1], tint[2] * DIP]),
        (0.5, tint),
        (1.0, [tint[0], tint[1] * DIP, tint[2] * DIP]),
    ])
}

fn texture(uri: &str) -> AssetRef {
    AssetRef::parse(uri).expect("built-in particle texture URI is valid")
}

#[cfg(test)]
#[path = "legacy/tests.rs"]
mod tests;
