use rbx_dom::Variant;

use super::*;

fn props(entries: Vec<(&str, Variant)>) -> Properties {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn at_origin() -> Origin {
    Origin::of_part(Mat4::IDENTITY)
}

#[test]
fn a_disabled_emitter_builds_nothing() {
    let mut budget = TOTAL_CAP;
    let properties = props(vec![("Enabled", Variant::Bool(false))]);
    assert!(build(&properties, at_origin(), Ref::new(1), &mut budget).is_none());
    assert_eq!(budget, TOTAL_CAP, "a skipped emitter spends no budget");
}

#[test]
fn an_emitter_with_no_properties_falls_back_to_documented_defaults() {
    let mut budget = TOTAL_CAP;
    let emitter = build(&props(vec![]), at_origin(), Ref::new(1), &mut budget).unwrap();
    assert_eq!(emitter.rate, 20.0);
    assert_eq!(emitter.lifetime, (5.0, 10.0));
    assert_eq!(emitter.texture, AssetRef::parse(DEFAULT_TEXTURE).unwrap());
    assert_eq!(
        emitter.direction,
        Vec3::Y,
        "default EmissionDirection is Top"
    );
    assert_eq!(emitter.time_scale, 1.0);
    assert_eq!(emitter.slot, 0);
}

#[test]
fn time_scale_is_clamped_to_the_documented_zero_to_one() {
    let mut budget = TOTAL_CAP;
    let frozen = props(vec![("TimeScale", Variant::Float32(0.0))]);
    let over = props(vec![("TimeScale", Variant::Float32(4.0))]);
    assert_eq!(
        build(&frozen, at_origin(), Ref::new(1), &mut budget)
            .unwrap()
            .time_scale,
        0.0
    );
    assert_eq!(
        build(&over, at_origin(), Ref::new(2), &mut budget)
            .unwrap()
            .time_scale,
        1.0
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
    let emitter = build(&properties, at_origin(), Ref::new(1), &mut budget).unwrap();
    assert_eq!(emitter.cap, 30);

    let huge = props(vec![
        ("Rate", Variant::Float32(1_000_000.0)),
        (
            "Lifetime",
            Variant::NumberRange(rbx_dom::NumberRange { min: 1.0, max: 1.0 }),
        ),
    ]);
    let mut budget = TOTAL_CAP;
    let emitter = build(&huge, at_origin(), Ref::new(2), &mut budget).unwrap();
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
    let first = build(&properties, at_origin(), Ref::new(1), &mut budget).unwrap();
    let second = build(&properties, at_origin(), Ref::new(2), &mut budget).unwrap();
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
    assert_ne!(seed_of(1, 0), seed_of(2, 0));
}

// A `Fire`'s two emitters share a referent, so the slot is the only thing
// keeping their particle streams — and the simulations a re-plan matches them
// to — apart.
#[test]
fn two_slots_of_one_instance_seed_different_streams() {
    assert_ne!(seed_of(7, 0), seed_of(7, 1));
}

// An `Attachment` parent is a point, not a volume: every particle starts at
// the attachment itself, however big the part holding it is.
#[test]
fn an_attachment_origin_spawns_at_its_own_point() {
    let cframe = Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0));
    let origin = Origin::of_attachment(cframe);
    for corner in [Vec3::splat(-0.5), Vec3::splat(0.5), Vec3::ZERO] {
        let spawn = origin.volume.transform_point3(corner);
        assert!((spawn - Vec3::new(3.0, 4.0, 5.0)).length() < 1e-5);
    }
    assert!((origin.up() - Vec3::Y).length() < 1e-5);
}

// A part parent emits from anywhere inside the part, which is what makes a
// `ParticleEmitter` on a wide part a wide sheet of particles.
#[test]
fn a_part_origin_spawns_across_the_whole_part() {
    let model = Mat4::from_scale(Vec3::new(10.0, 1.0, 1.0));
    let origin = Origin::of_part(model);
    let left = origin.volume.transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    let right = origin.volume.transform_point3(Vec3::new(0.5, 0.0, 0.0));
    assert!((right.x - left.x - 10.0).abs() < 1e-5);
    // The preconfigured classes are documented as emitting from the centre
    // of that same part instead.
    let centre = origin.centre().transform_point3(Vec3::new(-0.5, 0.0, 0.0));
    assert!(centre.length() < 1e-5);
}
