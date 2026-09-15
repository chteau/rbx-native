use super::*;
use rbx_dom::Instance;

/// A `Lighting` carrying one effect child per entry, in order.
fn lighting_with(effects: &[(&str, &[(&str, Variant)])]) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let lighting_ref = Ref::new(1);
    dom.insert(Instance::new(lighting_ref, "Lighting", "Lighting"));
    dom.set_parent(lighting_ref, None);

    for (index, (class, properties)) in effects.iter().enumerate() {
        let child = Ref::new(index as u32 + 2);
        let mut instance = Instance::new(child, *class, *class);
        for (name, value) in *properties {
            instance
                .properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        dom.insert(instance);
        dom.set_parent(child, Some(lighting_ref));
    }

    (dom, lighting_ref)
}

fn read_effects(effects: &[(&str, &[(&str, Variant)])]) -> Effects {
    let (dom, lighting_ref) = lighting_with(effects);
    read(&dom, &ReflectionDatabase::embedded(), lighting_ref)
}

#[test]
fn a_lighting_with_no_effects_at_all_gets_robloxs_own_bloom_defaults() {
    let effects = read_effects(&[]);

    assert_eq!(effects.bloom, Bloom::default());
    assert_eq!(effects.bloom.intensity, 0.4);
    assert_eq!(effects.bloom.size, 24.0);
    assert_eq!(effects.bloom.threshold, 0.95);
    assert_eq!(effects.blur, None);
    assert_eq!(effects.color_correction, None);
    assert_eq!(effects.tonemap, Tonemap::Default);
}

#[test]
fn a_bloom_effect_is_read_property_by_property() {
    let effects = read_effects(&[(
        "BloomEffect",
        &[
            ("Intensity", Variant::Float32(1.0)),
            ("Size", Variant::Float32(56.0)),
            ("Threshold", Variant::Float32(3.8)),
        ],
    )]);

    assert_eq!(effects.bloom.intensity, 1.0);
    assert_eq!(effects.bloom.size, 56.0);
    assert_eq!(effects.bloom.threshold, 3.8);
}

// `Enabled = false` is how a place keeps an effect around without it running.
#[test]
fn a_disabled_effect_is_skipped_and_the_next_enabled_one_wins() {
    let effects = read_effects(&[
        (
            "BloomEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("Intensity", Variant::Float32(9.0)),
            ],
        ),
        (
            "BloomEffect",
            &[
                ("Enabled", Variant::Bool(true)),
                ("Intensity", Variant::Float32(2.0)),
            ],
        ),
    ]);

    assert_eq!(effects.bloom.intensity, 2.0);
}

#[test]
fn a_disabled_colour_correction_leaves_no_grade_at_all() {
    let effects = read_effects(&[(
        "ColorCorrectionEffect",
        &[
            ("Enabled", Variant::Bool(false)),
            ("Saturation", Variant::Float32(1.0)),
        ],
    )]);

    assert_eq!(effects.color_correction, None);
}

// `Enabled` defaults to true and a place that never touched it does not write
// it out, so a property-less effect has to count as enabled.
#[test]
fn an_effect_that_never_serialized_enabled_still_runs() {
    let effects = read_effects(&[("ColorCorrectionEffect", &[])]);

    assert_eq!(
        effects.color_correction,
        Some(ColorCorrection {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            tint: Vec3::ONE,
        })
    );
}

#[test]
fn a_colour_correction_is_read_property_by_property() {
    let effects = read_effects(&[(
        "ColorCorrectionEffect",
        &[
            ("Brightness", Variant::Float32(0.1)),
            ("Contrast", Variant::Float32(0.1)),
            ("Saturation", Variant::Float32(0.05)),
            (
                "TintColor",
                Variant::Color3uint8 {
                    r: 255,
                    g: 128,
                    b: 0,
                },
            ),
        ],
    )]);

    let correction = effects.color_correction.expect("enabled");
    assert_eq!(correction.brightness, 0.1);
    assert_eq!(correction.contrast, 0.1);
    assert_eq!(correction.saturation, 0.05);
    // The tint is a gain, so it keeps the raw 0-1 components Studio shows
    // rather than being linearized like a radiance would be.
    assert!((correction.tint.y - 128.0 / 255.0).abs() < 1e-6);
}

#[test]
fn an_identity_grade_leaves_a_colour_exactly_where_it_was() {
    let identity = ColorCorrection {
        brightness: 0.0,
        contrast: 0.0,
        saturation: 0.0,
        tint: Vec3::ONE,
    };

    let color = Vec3::new(0.2, 0.5, 0.8);

    assert!((identity.apply(color) - color).length() < 1e-6);
}

// Brightness adds, contrast pushes away from the 0.5 pivot (above it rises),
// and saturation is a no-op on grey.
#[test]
fn a_grade_lifts_a_grey_and_leaves_it_grey() {
    let grade = ColorCorrection {
        brightness: 0.1,
        contrast: 0.1,
        saturation: 0.05,
        tint: Vec3::ONE,
    };

    let graded = grade.apply(Vec3::splat(0.5));

    // 0.5 -> 0.6 -> (0.6 - 0.5) * 1.1 + 0.5 = 0.61, and grey has no saturation
    // to stretch.
    assert!((graded.x - 0.61).abs() < 1e-5, "{graded:?}");
    assert!((graded.x - graded.y).abs() < 1e-6 && (graded.y - graded.z).abs() < 1e-6);
}

#[test]
fn contrast_pivots_around_the_middle_grey_rather_than_black() {
    let contrast = ColorCorrection {
        brightness: 0.0,
        contrast: 1.0,
        saturation: 0.0,
        tint: Vec3::ONE,
    };

    // Doubling the distance from 0.5: 0.25 falls to 0, 0.75 rises to 1.
    assert!((contrast.apply(Vec3::splat(0.25)).x - 0.0).abs() < 1e-6);
    assert!((contrast.apply(Vec3::splat(0.75)).x - 1.0).abs() < 1e-6);
    assert!((contrast.apply(Vec3::splat(0.5)).x - 0.5).abs() < 1e-6);
}

#[test]
fn full_desaturation_collapses_a_colour_onto_its_own_luma() {
    let grey = ColorCorrection {
        brightness: 0.0,
        contrast: 0.0,
        saturation: -1.0,
        tint: Vec3::ONE,
    };

    let color = Vec3::new(1.0, 0.0, 0.0);
    let graded = grey.apply(color);

    assert!((graded.x - LUMA.x).abs() < 1e-6, "{graded:?}");
    assert_eq!(graded.x, graded.z);
}

#[test]
fn a_tint_multiplies_before_anything_is_added() {
    let tinted = ColorCorrection {
        brightness: 0.5,
        contrast: 0.0,
        saturation: 0.0,
        tint: Vec3::new(0.0, 1.0, 1.0),
    };

    // Red is killed by the tint, then lifted by brightness like every other
    // channel — brightness after tint, not before.
    assert!((tinted.apply(Vec3::ONE).x - 0.5).abs() < 1e-6);
    assert!((tinted.apply(Vec3::ONE).y - 1.5).abs() < 1e-6);
}

#[test]
fn a_lighting_with_no_color_grading_effect_at_all_resolves_to_default() {
    let effects = read_effects(&[]);

    assert_eq!(effects.tonemap, Tonemap::Default);
}

// Roblox's own numbering: 0 is Default, 1 is Retro. A place that never touched
// the property does not serialize it, so a bare instance also has to land on
// Default.
#[test]
fn a_color_grading_effect_with_no_preset_resolves_to_default() {
    let effects = read_effects(&[("ColorGradingEffect", &[])]);

    assert_eq!(effects.tonemap, Tonemap::Default);
}

#[test]
fn a_color_grading_effect_asking_for_retro_gets_it() {
    let effects = read_effects(&[(
        "ColorGradingEffect",
        &[("TonemapperPreset", Variant::Enum(1))],
    )]);

    assert_eq!(effects.tonemap, Tonemap::Retro);
}

// Any value Roblox hasn't published a meaning for (here, a value past Retro)
// is refused rather than half-trusted: Default is the only safe fallback.
#[test]
fn a_color_grading_effect_with_an_unknown_preset_falls_back_to_default() {
    let effects = read_effects(&[(
        "ColorGradingEffect",
        &[("TonemapperPreset", Variant::Enum(7))],
    )]);

    assert_eq!(effects.tonemap, Tonemap::Default);
}

#[test]
fn a_disabled_color_grading_effect_is_skipped_entirely() {
    let effects = read_effects(&[(
        "ColorGradingEffect",
        &[
            ("Enabled", Variant::Bool(false)),
            ("TonemapperPreset", Variant::Enum(1)),
        ],
    )]);

    assert_eq!(effects.tonemap, Tonemap::Default);
}

#[test]
fn a_disabled_color_grading_effect_lets_the_next_enabled_one_win() {
    let effects = read_effects(&[
        (
            "ColorGradingEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("TonemapperPreset", Variant::Enum(1)),
            ],
        ),
        (
            "ColorGradingEffect",
            &[("TonemapperPreset", Variant::Enum(1))],
        ),
    ]);

    assert_eq!(effects.tonemap, Tonemap::Retro);
}

#[test]
fn a_lighting_with_no_blur_effect_at_all_has_no_blur() {
    let effects = read_effects(&[]);

    assert_eq!(effects.blur, None);
}

// A disabled blur effect must leave no blur at all, not fall back to a default.
#[test]
fn a_disabled_blur_effect_leaves_no_blur_at_all() {
    let effects = read_effects(&[(
        "BlurEffect",
        &[
            ("Enabled", Variant::Bool(false)),
            ("Size", Variant::Float32(12.0)),
        ],
    )]);

    assert_eq!(effects.blur, None);
}

#[test]
fn a_blur_effect_is_read_property_by_property() {
    let effects = read_effects(&[("BlurEffect", &[("Size", Variant::Float32(56.0))])]);

    assert_eq!(effects.blur, Some(Blur { size: 56.0 }));
}

// A place that never touched Size gets Roblox's own default of 24, the same
// number `BloomEffect.Size` defaults to.
#[test]
fn a_blur_effect_that_never_serialized_size_gets_robloxs_default() {
    let effects = read_effects(&[("BlurEffect", &[])]);

    assert_eq!(effects.blur, Some(Blur { size: 24.0 }));
}

#[test]
fn a_disabled_blur_effect_lets_the_next_enabled_one_win() {
    let effects = read_effects(&[
        (
            "BlurEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("Size", Variant::Float32(9.0)),
            ],
        ),
        ("BlurEffect", &[("Size", Variant::Float32(2.0))]),
    ]);

    assert_eq!(effects.blur, Some(Blur { size: 2.0 }));
}

// Roblox documents this specifically for BlurEffect (create.roblox.com/docs,
// BlurEffect.Size: "the instance with the greatest Size takes priority") —
// unlike every other effect here, it is not "whichever is enabled first".
#[test]
fn among_several_enabled_blur_effects_the_greatest_size_wins() {
    let effects = read_effects(&[
        ("BlurEffect", &[("Size", Variant::Float32(4.0))]),
        ("BlurEffect", &[("Size", Variant::Float32(56.0))]),
        ("BlurEffect", &[("Size", Variant::Float32(12.0))]),
    ]);

    assert_eq!(effects.blur, Some(Blur { size: 56.0 }));
}

// A disabled BlurEffect must lose the size comparison even when its own Size
// is the largest number on the list — it never competes at all.
#[test]
fn a_disabled_blur_effects_size_never_enters_the_comparison() {
    let effects = read_effects(&[
        (
            "BlurEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("Size", Variant::Float32(999.0)),
            ],
        ),
        ("BlurEffect", &[("Size", Variant::Float32(8.0))]),
    ]);

    assert_eq!(effects.blur, Some(Blur { size: 8.0 }));
}

// `SunRaysEffect` and `DepthOfFieldEffect` parsing have their own files — purely
// to keep this one under the workspace's 400-line file guideline.
#[path = "tests/sun_rays.rs"]
mod sun_rays;

#[path = "tests/depth_of_field.rs"]
mod depth_of_field;
