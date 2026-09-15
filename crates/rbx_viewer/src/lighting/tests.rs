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
    // Studio's fresh baseplate: 70/255 ambient, 14:30, no fog worth seeing.
    assert!(close(
        lighting.ambient,
        Vec3::splat(srgb_to_linear(DEFAULT_AMBIENT))
    ));
    assert!(close(
        lighting.sun_direction,
        sun_direction(DEFAULT_CLOCK_HOURS, 0.0)
    ));
    assert_eq!(lighting.exposure, 1.0);
    assert!(matches!(lighting.fog, Fog::Linear { end, .. } if end == DEFAULT_FOG_END));
}

#[test]
fn a_missing_property_falls_back_one_at_a_time() {
    let dom = dom_with(&[("Brightness", Variant::Float32(1.0))], &[]);

    let lighting = Lighting::from_dom(&dom, &database(), None);

    assert!(close(lighting.sun_color, Vec3::splat(SUN_BASE)));
    assert!(close(
        lighting.ambient,
        Vec3::splat(srgb_to_linear(DEFAULT_AMBIENT))
    ));
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
        ],
        &[],
    );

    let lighting = Lighting::from_dom(&dom, &database(), None);

    assert!(close(lighting.sun_color, Vec3::splat(2.0 * SUN_BASE)));
    assert!(close(
        lighting.fill_color,
        Vec3::splat(2.0 * SUN_BASE) * FILL_FRACTION * FILL_TINT
    ));
}

// The calibration the whole look hangs on. Studio's own capture puts a sunlit
// 163-grey top face on (175, 183, 196), i.e. 1.17 times its own linear colour in
// red; the two lamps and the grey ambient carry a shade under 1.0 of that and
// the sky's irradiance (in the shader, not here) makes up the rest. Any sun that
// reaches 1.17 on its own leaves no room for the sky and turns every top face
// white.
#[test]
fn the_sun_alone_falls_short_of_a_lit_top_face_by_the_sky_bounce() {
    let lighting = Lighting::default();

    let direct = lighting.sun_color * lighting.sun_direction.y.max(0.0);
    let lit = direct + lighting.ambient;

    assert!(lit.x > 0.9 && lit.x < 1.1, "{lit:?}");
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
    let place = Lighting::from_dom(&dom, &database, None);
    assert!(close(place.sun_direction, sun_direction(9.0, 0.0)));

    let overridden = Lighting::from_dom(&dom, &database, Some(3.0));
    assert!(close(overridden.sun_direction, sun_direction(3.0, 0.0)));
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
    // Colder than the daytime fill, and still bright enough to see by.
    assert!(night.fill_color.z > night.fill_color.x * 1.5);
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
    assert!(close(lighting.sun_color, Vec3::splat(3.0 * SUN_BASE)));
    assert!(close(lighting.sun_direction, sun_direction(14.5, 0.0)));
    assert!(lighting.sun_direction.y > 0.7);
    assert!(matches!(lighting.fog, Fog::Atmosphere { density, .. } if density == 0.3));
}

// `GlobalShadows` is only serialized when a place turns it off, so the default
// has to be on — and `ShadowSoftness` has to land on Studio's own 0.2 rather
// than on the hard edge a zero would give.
#[test]
fn shadows_default_to_on_and_to_studios_own_softness() {
    let lighting = Lighting::from_dom(&dom_with(&[], &[]), &database(), None);

    assert!(lighting.global_shadows);
    assert_eq!(lighting.shadow_softness, DEFAULT_SHADOW_SOFTNESS);
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
