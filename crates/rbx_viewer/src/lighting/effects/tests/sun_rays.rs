use super::*;

#[test]
fn a_lighting_with_no_sun_rays_effect_at_all_has_no_sun_rays() {
    let effects = read_effects(&[]);

    assert_eq!(effects.sun_rays, None);
}

// Parse sun rays properties: subtle intensity (0.01) and spread (0.1) values
// are easy to miss if rendered incorrectly.
#[test]
fn a_sun_rays_effect_is_read_property_by_property() {
    let effects = read_effects(&[(
        "SunRaysEffect",
        &[
            ("Intensity", Variant::Float32(0.01)),
            ("Spread", Variant::Float32(0.1)),
        ],
    )]);

    assert_eq!(
        effects.sun_rays,
        Some(SunRays {
            intensity: 0.01,
            spread: 0.1,
        })
    );
}

// No known fixture ever omits either property, so this is the fallback this
// renderer picked rather than one lifted from a captured default: no rays
// rather than guessed ones.
#[test]
fn a_sun_rays_effect_that_never_serialized_its_properties_renders_no_rays() {
    let effects = read_effects(&[("SunRaysEffect", &[])]);

    assert_eq!(
        effects.sun_rays,
        Some(SunRays {
            intensity: 0.0,
            spread: 0.0,
        })
    );
}

#[test]
fn a_disabled_sun_rays_effect_leaves_no_sun_rays_at_all() {
    let effects = read_effects(&[(
        "SunRaysEffect",
        &[
            ("Enabled", Variant::Bool(false)),
            ("Intensity", Variant::Float32(1.0)),
        ],
    )]);

    assert_eq!(effects.sun_rays, None);
}

#[test]
fn a_disabled_sun_rays_effect_lets_the_next_enabled_one_win() {
    let effects = read_effects(&[
        (
            "SunRaysEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("Intensity", Variant::Float32(9.0)),
            ],
        ),
        ("SunRaysEffect", &[("Intensity", Variant::Float32(0.5))]),
    ]);

    assert_eq!(
        effects.sun_rays,
        Some(SunRays {
            intensity: 0.5,
            spread: 0.0,
        })
    );
}
