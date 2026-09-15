use super::*;
use crate::camera::BACKGROUND_DISTANCE;

/// An enabled custom depth of field with extreme focus distance.
fn example_custom_dof() -> DepthOfField {
    DepthOfField {
        focus_distance: 0.01,
        in_focus_radius: 10.0,
        near_intensity: 0.75,
        far_intensity: 0.03,
    }
}

/// Readable numbers to exercise the ramp with: a sharp zone 20 studs wide
/// centred 50 studs out, so the near side ramps over 40-50 and the far side over
/// 60-80 (see `DOF_FALLOFF_RADII`).
fn studs_scale() -> DepthOfField {
    DepthOfField {
        focus_distance: 50.0,
        in_focus_radius: 20.0,
        near_intensity: 1.0,
        far_intensity: 0.5,
    }
}

#[test]
fn a_lighting_with_no_depth_of_field_effect_at_all_has_none() {
    let effects = read_effects(&[]);

    assert_eq!(effects.depth_of_field, None);
}

// A disabled depth of field effect must leave no blur at all, not fall back to
// a default focus plane.
#[test]
fn a_disabled_depth_of_field_effect_leaves_none_at_all() {
    let effects = read_effects(&[(
        "DepthOfFieldEffect",
        &[
            ("Enabled", Variant::Bool(false)),
            ("FocusDistance", Variant::Float32(0.05)),
            ("InFocusRadius", Variant::Float32(30.0)),
            ("NearIntensity", Variant::Float32(0.75)),
            ("FarIntensity", Variant::Float32(0.1)),
        ],
    )]);

    assert_eq!(effects.depth_of_field, None);
}

// Parse properties correctly into the effect structure.
#[test]
fn a_depth_of_field_effect_is_read_property_by_property() {
    let effects = read_effects(&[(
        "DepthOfFieldEffect",
        &[
            ("Enabled", Variant::Bool(true)),
            ("FocusDistance", Variant::Float32(0.01)),
            ("InFocusRadius", Variant::Float32(10.0)),
            ("NearIntensity", Variant::Float32(0.75)),
            ("FarIntensity", Variant::Float32(0.03)),
        ],
    )]);

    assert_eq!(effects.depth_of_field, Some(example_custom_dof()));
}

// A place that never touched a property gets the number a fresh Studio place
// serializes, which for this effect is a known capture rather than a guess.
#[test]
fn a_depth_of_field_effect_that_serialized_nothing_gets_studios_own_defaults() {
    let effects = read_effects(&[("DepthOfFieldEffect", &[])]);

    assert_eq!(
        effects.depth_of_field,
        Some(DepthOfField {
            focus_distance: 0.05,
            in_focus_radius: 30.0,
            near_intensity: 0.75,
            far_intensity: 0.1,
        })
    );
}

#[test]
fn a_disabled_depth_of_field_effect_lets_the_next_enabled_one_win() {
    let effects = read_effects(&[
        (
            "DepthOfFieldEffect",
            &[
                ("Enabled", Variant::Bool(false)),
                ("FocusDistance", Variant::Float32(9.0)),
            ],
        ),
        (
            "DepthOfFieldEffect",
            &[("FocusDistance", Variant::Float32(2.0))],
        ),
    ]);

    assert_eq!(
        effects.depth_of_field.map(|dof| dof.focus_distance),
        Some(2.0)
    );
}

// Studio's sliders stop at 1 and a negative distance is meaningless, so a
// hand-edited or script-written instance is clamped rather than trusted.
#[test]
fn out_of_range_properties_are_clamped_rather_than_trusted() {
    let effects = read_effects(&[(
        "DepthOfFieldEffect",
        &[
            ("FocusDistance", Variant::Float32(-5.0)),
            ("InFocusRadius", Variant::Float32(-30.0)),
            ("NearIntensity", Variant::Float32(4.0)),
            ("FarIntensity", Variant::Float32(-1.0)),
        ],
    )]);

    assert_eq!(
        effects.depth_of_field,
        Some(DepthOfField {
            focus_distance: 0.0,
            in_focus_radius: 0.0,
            near_intensity: 1.0,
            far_intensity: 0.0,
        })
    );
}

// The sharp zone is exactly that: no mix of the blurred copy at all, not a small
// one, or a place with a wide InFocusRadius would read as softer everywhere.
// Roblox's own docs for `InFocusRadius`: "the distance away from FocusDistance
// (on both sides) where no blur is applied" — so with focus 50 and radius 20,
// the zone is the full [30, 70], not the halved [40, 60].
#[test]
fn the_in_focus_zone_is_exactly_sharp_out_to_the_full_radius() {
    let dof = studs_scale();

    for distance in [30.0, 40.0, 50.0, 60.0, 70.0] {
        assert_eq!(dof.blur_factor(distance), 0.0, "{distance} studs");
    }
}

// Near and far are independent properties, so a distance either side of the same
// focus plane at the same offset must not blur by the same amount.
#[test]
fn each_side_of_the_focus_plane_ramps_to_its_own_intensity() {
    let dof = studs_scale();

    // 10 studs past the sharp edge (30 and 70), i.e. halfway along a 20-stud
    // falloff.
    assert!((dof.blur_factor(20.0) - 0.5).abs() < 1e-6);
    assert!((dof.blur_factor(80.0) - 0.25).abs() < 1e-6);
}

#[test]
fn the_ramp_clamps_at_each_sides_own_intensity() {
    let dof = studs_scale();

    // A full falloff past the sharp edge (20 studs past 30, and past 70) and
    // everything beyond it.
    assert!((dof.blur_factor(10.0) - 1.0).abs() < 1e-6);
    assert!((dof.blur_factor(0.0) - 1.0).abs() < 1e-6);
    assert!((dof.blur_factor(90.0) - 0.5).abs() < 1e-6);
    assert!((dof.blur_factor(100_000.0) - 0.5).abs() < 1e-6);
}

// A pixel nothing was drawn into is reconstructed at `BACKGROUND_DISTANCE` (see
// `camera`), which must land on the far intensity and produce a real number
// rather than a NaN out of an infinity.
#[test]
fn the_cleared_background_lands_on_the_far_intensity() {
    let dof = studs_scale();
    let factor = dof.blur_factor(BACKGROUND_DISTANCE);

    assert!(factor.is_finite());
    assert!((factor - dof.far_intensity).abs() < 1e-6, "{factor}");
}

// Extreme settings: focus plane 0.01 studs out means the near side is
// unreachable and the far side is a 3% mix. The sharp zone reaches focus +
// radius = 10.01 studs (InFocusRadius is not halved), so 10.0 studs stays sharp.
#[test]
fn extreme_dof_settings_blur_only_the_far_side_and_only_slightly() {
    let dof = example_custom_dof();

    assert_eq!(dof.blur_factor(1.0), 0.0);
    assert_eq!(dof.blur_factor(5.0), 0.0);
    assert_eq!(dof.blur_factor(10.0), 0.0);
    // Halfway along the 10-stud far falloff that starts at the 10.01 edge.
    assert!(
        (dof.blur_factor(15.01) - 0.03 * 0.5).abs() < 1e-3,
        "{dof:?}"
    );
    assert!((dof.blur_factor(100.0) - 0.03).abs() < 1e-6);
}

// InFocusRadius = 0 has no falloff length to divide by, so the whole ramp has to
// collapse to a hard step at the focus plane instead of a division by zero.
#[test]
fn a_zero_radius_is_a_hard_step_rather_than_a_nan() {
    let dof = DepthOfField {
        focus_distance: 10.0,
        in_focus_radius: 0.0,
        near_intensity: 0.8,
        far_intensity: 0.2,
    };

    assert_eq!(dof.blur_factor(10.0), 0.0);
    assert!((dof.blur_factor(9.0) - 0.8).abs() < 1e-6);
    assert!((dof.blur_factor(11.0) - 0.2).abs() < 1e-6);
}
