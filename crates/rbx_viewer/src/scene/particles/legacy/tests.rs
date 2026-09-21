use std::collections::HashMap;

use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Color3Data, Instance, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::scene::particles::sequence::eval_number;
use crate::scene::{Placement, ShapeKind};

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const PART: u32 = 2;
const EFFECT: u32 = 3;
const ATTACHMENT: u32 = 4;

fn cframe_at(position: Vec3) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation: IDENTITY_ROTATION,
    })
}

fn insert(
    dom: &mut WeakDom,
    id: u32,
    class: &str,
    parent: Option<Ref>,
    properties: Vec<(&str, Variant)>,
) -> Ref {
    let referent = Ref::new(id);
    let mut instance = Instance::new(referent, class, class);
    for (name, value) in properties {
        instance.properties_mut().insert(name.to_string(), value);
    }
    dom.insert(instance);
    dom.set_parent(referent, parent);
    referent
}

/// A place holding one part with `class` parented to it, and the placements
/// the plan resolves the part through.
fn place(class: &str, properties: Vec<(&str, Variant)>) -> (WeakDom, HashMap<Ref, Placement>) {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace", None, vec![]);
    let part = insert(
        &mut dom,
        PART,
        "Part",
        Some(workspace),
        vec![
            (
                "size",
                Variant::Vector3(Vector3Data {
                    x: 4.0,
                    y: 1.0,
                    z: 2.0,
                }),
            ),
            ("CFrame", cframe_at(Vec3::ZERO)),
        ],
    );
    insert(&mut dom, EFFECT, class, Some(part), properties);

    let placements = HashMap::from([(
        part,
        Placement {
            kind: ShapeKind::Box,
            model: Mat4::from_scale(Vec3::new(4.0, 1.0, 2.0)),
            size: Vec3::new(4.0, 1.0, 2.0),
        },
    )]);
    (dom, placements)
}

fn plan_of(class: &str, properties: Vec<(&str, Variant)>) -> Vec<Emitter> {
    let (dom, placements) = place(class, properties);
    crate::scene::particles::plan(&dom, &ReflectionDatabase::embedded(), &placements)
}

fn colour(r: f32, g: f32, b: f32) -> Variant {
    Variant::Color3(Color3Data { r, g, b })
}

/// The whole documented shape of a `Fire`: two emitters, the secondary one
/// smaller, longer-lived, rising farther, accelerated by `Heat` and at
/// `LightEmission` 1.
#[test]
fn a_fire_plans_the_two_emitters_the_docs_describe() {
    let emitters = plan_of("Fire", vec![("Heat", Variant::Float32(10.0))]);
    assert_eq!(emitters.len(), 2);
    let (outer, inner) = (&emitters[0], &emitters[1]);

    assert_eq!(outer.referent, inner.referent);
    assert_ne!(outer.slot, inner.slot, "a re-plan tells the two apart");
    assert_ne!(outer.seed, inner.seed, "and they are not one stream twice");

    assert!(inner.lifetime.1 > outer.lifetime.1);
    assert!(
        inner.speed.1 > outer.speed.1,
        "the inner flame rises farther"
    );
    assert!(eval_number(&inner.size, 0.35) < eval_number(&outer.size, 0.35));
    assert_eq!(inner.light_emission, 1.0);
    assert_eq!(outer.acceleration, Vec3::ZERO);
    assert!(
        inner.acceleration.y > 0.0,
        "Heat accelerates the inner particles"
    );
}

/// Documented: positive `Heat` emits up, negative down.
#[test]
fn negative_heat_turns_a_fire_upside_down() {
    let up = plan_of("Fire", vec![("Heat", Variant::Float32(9.0))]);
    let down = plan_of("Fire", vec![("Heat", Variant::Float32(-9.0))]);
    assert!((up[0].direction - Vec3::Y).length() < 1e-5);
    assert!((down[0].direction - Vec3::NEG_Y).length() < 1e-5);
    // Speed is a magnitude either way: a downward fire is not a negative one.
    assert!(down[0].speed.0 >= 0.0 && down[0].speed.1 > 0.0);
}

/// Documented: `Size` is limited to 2–30, and the flames come out somewhat
/// smaller than that in studs.
#[test]
fn fire_size_is_clamped_to_its_documented_range() {
    let huge = plan_of("Fire", vec![("Size", Variant::Float32(1000.0))]);
    let at_ceiling = plan_of("Fire", vec![("Size", Variant::Float32(30.0))]);
    assert_eq!(
        eval_number(&huge[0].size, 0.35),
        eval_number(&at_ceiling[0].size, 0.35)
    );
    assert!(eval_number(&at_ceiling[0].size, 0.35) < 30.0);
}

/// The legacy lowercase spelling the API dump lists beside `Fire.Size`.
#[test]
fn fire_reads_the_lowercase_size_a_file_may_carry() {
    let lower = plan_of("Fire", vec![("size", Variant::Float32(20.0))]);
    let upper = plan_of("Fire", vec![("Size", Variant::Float32(20.0))]);
    assert_eq!(
        eval_number(&lower[0].size, 0.35),
        eval_number(&upper[0].size, 0.35)
    );
}

/// Documented: `Opacity` works inversely to `Transparency` — 0 invisible, 1
/// visible.
#[test]
fn smoke_opacity_is_the_inverse_of_transparency() {
    let solid = plan_of("Smoke", vec![("Opacity", Variant::Float32(1.0))]);
    let gone = plan_of("Smoke", vec![("Opacity", Variant::Float32(0.0))]);
    assert_eq!(eval_number(&solid[0].transparency, 0.0), 0.0);
    assert_eq!(eval_number(&gone[0].transparency, 0.0), 1.0);
}

/// Documented: smoke particles are "more than twice as large" as `Size` in
/// studs, and at the largest size render "larger than 200 studs wide".
#[test]
fn smoke_particles_are_more_than_twice_their_size_in_studs() {
    let one = plan_of("Smoke", vec![("Size", Variant::Float32(1.0))]);
    assert!(eval_number(&one[0].size, 1.0) > 2.0);
    let largest = plan_of("Smoke", vec![("Size", Variant::Float32(100.0))]);
    assert!(eval_number(&largest[0].size, 1.0) > 200.0);
}

/// Documented: negative `RiseVelocity` emits downward.
#[test]
fn negative_rise_velocity_sends_smoke_downward() {
    let down = plan_of("Smoke", vec![("RiseVelocity", Variant::Float32(-6.0))]);
    assert!((down[0].direction - Vec3::NEG_Y).length() < 1e-5);
    assert!(down[0].speed.1 > 0.0);
}

/// `SparkleColor` and its deprecated twin `Color` are documented as doing
/// the exact same thing, and an older file carries the latter.
#[test]
fn sparkles_reads_either_colour_property() {
    let live = plan_of("Sparkles", vec![("SparkleColor", colour(1.0, 0.0, 0.0))]);
    let legacy = plan_of("Sparkles", vec![("Color", colour(1.0, 0.0, 0.0))]);
    let midpoint = |emitter: &Emitter| crate::scene::eval_color(&emitter.color, 0.5);
    assert_eq!(midpoint(&live[0]), midpoint(&legacy[0]));
    assert_eq!(midpoint(&live[0]), [1.0, 0.0, 0.0]);
}

/// Documented: the sparkle colour "very faintly animate[s] between a subtle
/// green and red".
#[test]
fn sparkles_twinkle_between_green_and_red() {
    let emitters = plan_of("Sparkles", vec![("SparkleColor", colour(1.0, 1.0, 1.0))]);
    // The keypoints themselves rather than `eval_color`, which answers in
    // linear light: "faintly" is a statement about the colour as authored.
    let at = |index: usize| {
        let point = emitters[0].color.keypoints[index].color;
        [point.r, point.g, point.b]
    };
    let (start, end) = (at(0), at(2));
    assert!(start[1] > start[0] && start[1] > start[2], "green first");
    assert!(end[0] > end[1] && end[0] > end[2], "red last");
    assert!(start[1] - start[0] < 0.2, "faintly");
}

#[test]
fn a_disabled_preconfigured_effect_plans_nothing() {
    for class in ["Fire", "Smoke", "Sparkles"] {
        let emitters = plan_of(class, vec![("Enabled", Variant::Bool(false))]);
        assert!(emitters.is_empty(), "{class}");
    }
}

/// All three are documented as emitting from the centre of the part, unlike
/// a `ParticleEmitter`, which emits from anywhere inside it.
#[test]
fn a_preconfigured_effect_emits_from_the_centre_of_its_part() {
    let emitters = plan_of("Smoke", vec![]);
    let spawn = emitters[0].volume.transform_point3(Vec3::splat(0.5));
    assert!(spawn.length() < 1e-5);
}

/// Every one of the three documents an `Attachment` parent as the way to
/// move the emission point and direction off the part's own centre.
#[test]
fn an_effect_on_an_attachment_emits_from_the_attachment() {
    let (mut dom, placements) = place("Fire", vec![]);
    let part = Ref::new(PART);
    let attachment = insert(
        &mut dom,
        ATTACHMENT,
        "Attachment",
        Some(part),
        vec![("CFrame", cframe_at(Vec3::new(0.0, 7.0, 0.0)))],
    );
    dom.set_parent(Ref::new(EFFECT), Some(attachment));

    let emitters =
        crate::scene::particles::plan(&dom, &ReflectionDatabase::embedded(), &placements);
    assert_eq!(emitters.len(), 2);
    let spawn = emitters[0].volume.transform_point3(Vec3::splat(0.5));
    assert!((spawn - Vec3::new(0.0, 7.0, 0.0)).length() < 1e-5);
}

/// Each effect draws through Roblox's own image for it, so the loader fetches
/// three distinct textures rather than reusing the emitter default for all.
#[test]
fn each_effect_names_its_own_texture() {
    let fire = plan_of("Fire", vec![]).remove(0).texture;
    let smoke = plan_of("Smoke", vec![]).remove(0).texture;
    let sparkles = plan_of("Sparkles", vec![]).remove(0).texture;
    assert_eq!(fire, AssetRef::parse(FIRE_TEXTURE).unwrap());
    assert_eq!(smoke, AssetRef::parse(SMOKE_TEXTURE).unwrap());
    assert_eq!(sparkles, AssetRef::parse(SPARKLES_TEXTURE).unwrap());
    assert_ne!(fire, smoke);
}
