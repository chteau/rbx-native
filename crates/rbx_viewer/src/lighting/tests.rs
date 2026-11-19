use rbx_dom::{Instance, Ref};

use super::*;

const EPSILON: f32 = 1e-4;

fn close(actual: Vec3, expected: Vec3) -> bool {
    (actual - expected).length() < EPSILON
}

/// A DOM holding one `Lighting` with the given properties, optionally with an
/// `Atmosphere` child.
fn dom_with(properties: &[(&str, Variant)], atmosphere: &[(&str, Variant)]) -> WeakDom {
    let mut dom = WeakDom::new();
    let lighting_ref = Ref::new(1);
    let mut lighting = Instance::new(lighting_ref, "Lighting", "Lighting");
    for (name, value) in properties {
        lighting
            .properties_mut()
            .insert((*name).to_string(), value.clone());
    }
    dom.insert(lighting);
    dom.set_parent(lighting_ref, None);

    if !atmosphere.is_empty() {
        let child_ref = Ref::new(2);
        let mut child = Instance::new(child_ref, "Atmosphere", "Atmosphere");
        for (name, value) in atmosphere {
            child
                .properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        dom.insert(child);
        dom.set_parent(child_ref, Some(lighting_ref));
    }
    dom
}

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

// The three angles the community formula can be checked against without a
// reference implementation: they are the ones anyone can name from memory.
#[test]
fn noon_at_the_tropic_puts_the_sun_straight_overhead() {
    assert!(close(sun_direction(12.0, 23.5), Vec3::Y));
}

#[test]
fn sunrise_and_sunset_lie_on_the_horizon_on_opposite_sides() {
    assert!(close(sun_direction(6.0, 23.5), Vec3::X));
    assert!(close(sun_direction(18.0, 23.5), -Vec3::X));
}

#[test]
fn midnight_puts_the_sun_below_the_horizon() {
    assert!(sun_direction(0.0, 0.0).y < -0.5);
}

#[test]
fn the_clock_wraps_rather_than_running_off_the_day() {
    assert!(close(sun_direction(36.0, 23.5), sun_direction(12.0, 23.5)));
    assert!(close(sun_direction(-12.0, 23.5), sun_direction(12.0, 23.5)));
}

#[test]
fn every_direction_is_a_unit_vector() {
    for hour in 0..24 {
        let direction = sun_direction(hour as f32, 47.0);
        assert!((direction.length() - 1.0).abs() < EPSILON, "{hour}h");
    }
}

#[test]
fn a_time_of_day_string_reads_as_fractional_hours() {
    assert_eq!(parse_clock("14:30:00"), Some(14.5));
    assert_eq!(parse_clock("00:00:00"), Some(0.0));
    assert_eq!(parse_clock("6"), Some(6.0));
    assert_eq!(parse_clock("18:15"), Some(18.25));
    assert_eq!(parse_clock(" 9 : 30 "), Some(9.5));
}

#[test]
fn a_garbled_or_missing_time_of_day_is_refused_whole() {
    assert_eq!(parse_clock(""), None);
    assert_eq!(parse_clock("noon"), None);
    assert_eq!(parse_clock("14:xx:00"), None);
    assert_eq!(parse_clock("1:2:3:4"), None);
    assert_eq!(parse_clock("inf"), None);
}

#[test]
fn a_dom_without_a_lighting_service_still_lights_like_studio() {
    let mut dom = WeakDom::new();
    let part_ref = Ref::new(1);
    dom.insert(Instance::new(part_ref, "Part", "Part"));
    dom.set_parent(part_ref, None);

    let lighting = Lighting::from_dom(&dom, &database(), None);

    assert_eq!(lighting, Lighting::default());
    // Studio's fresh baseplate: 70/255 ambient under ShadowMap's scale, 14:30,
    // no fog worth seeing.
    assert!(close(
        lighting.ambient,
        Vec3::splat(srgb_to_linear(DEFAULT_AMBIENT) * FILMIC_AMBIENT)
    ));
    assert!(close(
        lighting.sun_direction,
        sun_direction(DEFAULT_CLOCK_HOURS, 0.0)
    ));
    assert_eq!(lighting.exposure, 1.0);
    assert!(matches!(lighting.fog, Fog::Linear { end, .. } if end == DEFAULT_FOG_END));
}

/// What the sun lamp is at `brightness` and `direction` with no colour
/// shift: the technology's base, the air it crosses.
fn sun(base: f32, brightness: f32, direction: Vec3) -> Vec3 {
    Vec3::splat(base * brightness) * sun::transmittance(direction.y)
}

// What a place leaves out is the `Lighting` class default (the real client's
// `default_lighting` fidelity scene): Voxel, 127 grey ambient, 14:00 at
// latitude 41.7, no shadows.
#[test]
fn a_missing_property_falls_back_one_at_a_time() {
    let dom = dom_with(&[("Brightness", Variant::Float32(1.0))], &[]);

    let lighting = Lighting::from_dom(&dom, &database(), None);

    assert!(close(
        lighting.sun_color,
        sun(VOXEL_SUN, 1.0, lighting.sun_direction)
    ));
    assert!(close(
        lighting.sun_direction,
        sun_direction(14.0, Defaults::class(&database()).latitude)
    ));
    assert!(lighting.voxel_locals);
    assert!((lighting.ambient.x - srgb_to_linear(0.5)).abs() < 0.01);
    assert!(!lighting.global_shadows);
}

// Lighting.OutdoorAmbient's docs clamp the effective value to at least Ambient
// per channel. marked.rbxl's Ambient (139) sits above its OutdoorAmbient (70),
// and its shadow sides rendered twice as dark as Studio's until this held.
#[test]
fn an_ambient_above_outdoor_ambient_lifts_the_outdoors_too() {
    let dom = dom_with(
        &[
            (
                "OutdoorAmbient",
                Variant::Color3uint8 {
                    r: 70,
                    g: 70,
                    b: 70,
                },
            ),
            (
                "Ambient",
                Variant::Color3uint8 {
                    r: 139,
                    g: 40,
                    b: 70,
                },
            ),
        ],
        &[],
    );

    let lighting = Lighting::from_dom(&dom, &database(), None);

    let expected = Vec3::from([139.0, 70.0, 70.0].map(|c| srgb_to_linear(c / 255.0)));
    assert!(close(lighting.ambient, expected), "{:?}", lighting.ambient);
}

#[test]
fn brightness_scales_the_sun_and_its_fill_together() {
    let dom = dom_with(
        &[
            ("Brightness", Variant::Float32(2.0)),
            ("TimeOfDay", Variant::String("12:00:00".to_string())),
            ("GeographicLatitude", Variant::Float32(23.5)),
            ("Technology", Variant::Enum(3)),
        ],
        &[],
    );

    let lighting = Lighting::from_dom(&dom, &database(), None);

    // Straight overhead: no air to lose any of it to.
    assert!(close(lighting.sun_color, Vec3::splat(2.0 * FILMIC_SUN)));
    assert!(close(
        lighting.fill_color,
        Vec3::splat(2.0 * FILMIC_SUN) * FILL_FRACTION * FILL_TINT
    ));
}

// The real client's tone ramp: a 136-grey top face under an overhead sun at
// Brightness 2 in a ShadowMap place lands on 146 once the filmic curve has
// run, which is the sun lamp at 0.774 of the face's own linear colour.
#[test]
fn an_overhead_sun_at_brightness_two_is_the_measured_lamp() {
    let lighting = Lighting::from_dom(
        &dom_with(
            &[
                ("Brightness", Variant::Float32(2.0)),
                ("ClockTime", Variant::Float32(12.0)),
                ("GeographicLatitude", Variant::Float32(23.5)),
                ("Technology", Variant::Enum(3)),
            ],
            &[],
        ),
        &database(),
        None,
    );

    let lit = lighting.sun_color * lighting.sun_direction.y.max(0.0);

    assert!((lit.x - 0.774).abs() < 0.01, "{lit:?}");
    assert_eq!(lighting.effects.tonemap, Tonemap::Filmic);
}

// Voxel, the class default, shows no curve at all.
#[test]
fn voxel_keeps_the_plain_clamp() {
    let lighting = Lighting::from_dom(&dom_with(&[], &[]), &database(), None);

    assert_eq!(lighting.effects.tonemap, Tonemap::Default);
}

#[test]
fn a_clock_time_override_beats_both_serialized_forms() {
    let dom = dom_with(
        &[
            ("TimeOfDay", Variant::String("14:30:00".to_string())),
            ("ClockTime", Variant::Float32(9.0)),
        ],
        &[],
    );
    let database = database();

    // ClockTime wins over TimeOfDay, and --clock-time wins over both.
    let latitude = Defaults::class(&database).latitude;
    let place = Lighting::from_dom(&dom, &database, None);
    assert!(close(place.sun_direction, sun_direction(9.0, latitude)));

    let overridden = Lighting::from_dom(&dom, &database, Some(3.0));
    assert!(close(overridden.sun_direction, sun_direction(3.0, latitude)));
}

#[test]
fn the_override_applies_even_with_no_lighting_service_to_override() {
    let dom = WeakDom::new();

    let lighting = Lighting::from_dom(&dom, &database(), Some(6.0));

    assert!(close(lighting.sun_direction, sun_direction(6.0, 0.0)));
}

// Twilight: the sun lamp fades out as it sets and the lamp behind it turns into
// moonlight, so nothing swaps direction in a single frame.
#[test]
fn the_sun_hands_over_to_the_moon_across_the_horizon() {
    let noon = Lighting::from_dom(
        &dom_with(&[("ClockTime", Variant::Float32(12.0))], &[]),
        &database(),
        None,
    );
    let night = Lighting::from_dom(
        &dom_with(&[("ClockTime", Variant::Float32(0.0))], &[]),
        &database(),
        None,
    );

    assert!(noon.sun_color.length() > night.sun_color.length());
    assert_eq!(night.sun_color, Vec3::ZERO);
    // White, as the real client's midnight shows, and bright enough to see by.
    assert_eq!(night.fill_color.x, night.fill_color.z);
    assert!(night.fill_color.z > 0.0);
}

#[test]
fn an_atmosphere_child_replaces_the_classic_fog() {
    let dom = dom_with(
        &[("FogEnd", Variant::Float32(500.0))],
        &[
            ("Density", Variant::Float32(0.4)),
            ("Offset", Variant::Float32(0.25)),
        ],
    );

    let lighting = Lighting::from_dom(&dom, &database(), None);

    match lighting.fog {
        Fog::Atmosphere {
            density, offset, ..
        } => {
            assert_eq!(density, 0.4);
            assert_eq!(offset, 0.25);
        }
        other => panic!("expected an atmosphere, got {other:?}"),
    }
}

#[test]
fn fog_end_can_never_fall_behind_fog_start() {
    let dom = dom_with(
        &[
            ("FogStart", Variant::Float32(400.0)),
            ("FogEnd", Variant::Float32(100.0)),
        ],
        &[],
    );

    match Lighting::from_dom(&dom, &database(), None).fog {
        Fog::Linear { start, end, .. } => assert!(end > start),
        other => panic!("expected linear fog, got {other:?}"),
    }
}

#[test]
fn the_test_place_fixture_reads_its_own_lighting() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let bytes = std::fs::read(&path).expect("fixture must be readable");
    let dom = rbx_binary::deserialize(&bytes).expect("fixture must parse");

    let lighting = Lighting::from_dom(&dom, &database(), None);

    // Brightness 3, TimeOfDay 14:30, latitude 0, and an Atmosphere at 0.3.
    let base = if lighting.effects.tonemap == Tonemap::Filmic {
        FILMIC_SUN
    } else {
        VOXEL_SUN
    };
    assert!(close(
        lighting.sun_color,
        sun(base, 3.0, lighting.sun_direction)
    ));
    assert!(close(lighting.sun_direction, sun_direction(14.5, 0.0)));
    assert!(lighting.sun_direction.y > 0.7);
    assert!(matches!(lighting.fog, Fog::Atmosphere { density, .. } if density == 0.3));
}

// A `Lighting` that leaves them out has the class defaults, shadows off and a
// softness of 0.5; with no `Lighting` at all it is Studio's template, shadows
// on at 0.2.
#[test]
fn shadows_default_to_the_class_and_the_template_to_studios_own() {
    let lighting = Lighting::from_dom(&dom_with(&[], &[]), &database(), None);
    assert!(!lighting.global_shadows);
    assert_eq!(lighting.shadow_softness, 0.5);

    let template = Lighting::default();
    assert!(template.global_shadows);
    assert_eq!(template.shadow_softness, DEFAULT_SHADOW_SOFTNESS);
}

#[test]
fn a_place_can_turn_shadows_off_and_pick_its_own_softness() {
    let dom = dom_with(
        &[
            ("GlobalShadows", Variant::Bool(false)),
            ("ShadowSoftness", Variant::Float32(0.75)),
        ],
        &[],
    );

    let lighting = Lighting::from_dom(&dom, &database(), None);

    assert!(!lighting.global_shadows);
    assert_eq!(lighting.shadow_softness, 0.75);
}

// The shader reads the softness as a fraction of one kernel radius, so anything
// outside 0..1 would stretch the PCF grid past the taps that cover it.
#[test]
fn an_out_of_range_softness_is_clamped_rather_than_trusted() {
    let soft = dom_with(&[("ShadowSoftness", Variant::Float32(9.0))], &[]);
    let negative = dom_with(&[("ShadowSoftness", Variant::Float32(-3.0))], &[]);

    assert_eq!(
        Lighting::from_dom(&soft, &database(), None).shadow_softness,
        1.0
    );
    assert_eq!(
        Lighting::from_dom(&negative, &database(), None).shadow_softness,
        0.0
    );
}

#[test]
fn fullbright_is_flat_and_shadowless_but_keeps_the_sky() {
    let night = Lighting::from_clock(Some(0.0), None);
    let lit = night.fullbright();
    assert!(!lit.global_shadows);
    assert_eq!(lit.environment_diffuse, 0.0);
    assert_eq!(lit.environment_specular, 0.0);
    assert_eq!(lit.ambient + lit.sun_color, Vec3::ONE);
    assert_eq!(lit.sun_color, lit.fill_color);
    assert!(matches!(lit.fog, Fog::Linear { start, .. } if start >= NO_FOG));
    assert_eq!(lit.sun_direction, night.sun_direction);
    assert_eq!(lit.sky_tint, night.sky_tint);
    assert_eq!(lit.star_fade, night.star_fade);
}
