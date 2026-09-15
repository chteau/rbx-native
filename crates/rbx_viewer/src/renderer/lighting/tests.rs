use super::*;
use crate::lighting::Clouds;
use crate::quality::QualityLevel;

/// The top level, which scales nothing down: these tests are about what the
/// place asks for, not about what a quality level takes away.
fn top() -> QualityProfile {
    QualityLevel::default().profile()
}

fn probe() -> Probe {
    Probe {
        top_mip: 4.0,
        irradiance: [[0.25, 0.5, 0.75, 0.0]; 6],
    }
}

/// The uniform as a place with no shadows at all packs it.
fn unshadowed(lighting: &Lighting, camera: Vec3) -> LightingRaw {
    LightingRaw::new(
        lighting,
        camera,
        &probe(),
        (Lamp::None, &Fit::unfitted()),
        0,
        &top(),
    )
}

#[test]
fn the_uniform_is_twenty_eight_vec4s_and_nothing_else() {
    // What `LightingUniform` in lighting.wgsl declares. A mismatch here is a
    // silently misread uniform buffer, not a compile error.
    assert_eq!(LightingRaw::SIZE, 28 * 16);
}

// An unlimited render distance has to reach the shader as 0 and not as an
// infinity: the shader subtracts and divides by it.
#[test]
fn the_quality_row_flattens_an_unlimited_render_distance_to_zero() {
    let top = unshadowed(&Lighting::default(), Vec3::ZERO);
    assert_eq!(top.quality[0], 0.0);
    assert_eq!(top.quality[1], 1.0);

    let low = LightingRaw::new(
        &Lighting::default(),
        Vec3::ZERO,
        &probe(),
        (Lamp::None, &Fit::unfitted()),
        0,
        &QualityLevel::Level(1).profile(),
    );

    assert_eq!(low.quality[0], 500.0);
    assert_eq!(low.quality[1], 0.0);
}

#[test]
fn linear_fog_and_an_atmosphere_are_told_apart_by_one_marker() {
    let linear = unshadowed(&Lighting::default(), Vec3::ZERO);
    assert_eq!(linear.fog_color[3], 0.0);

    let lighting = Lighting {
        fog: Fog::Atmosphere {
            color: Vec3::ONE,
            decay: Vec3::ZERO,
            density: 0.3,
            offset: 0.25,
            glare: 0.0,
            haze: 0.0,
        },
        ..Lighting::default()
    };
    let atmosphere = unshadowed(&lighting, Vec3::ZERO);

    assert_eq!(atmosphere.fog_color[3], 1.0);
    assert_eq!(atmosphere.fog_range[2], 0.3);
    assert_eq!(atmosphere.fog_range[3], 0.25);
}

#[test]
fn the_camera_and_the_tuning_row_carry_what_the_shader_indexes() {
    let lighting = Lighting {
        exposure: 2.0,
        environment_specular: 0.5,
        environment_diffuse: 0.25,
        ..Lighting::default()
    };

    let raw = unshadowed(&lighting, Vec3::new(1.0, 2.0, 3.0));

    assert_eq!(raw.camera, [1.0, 2.0, 3.0, 0.0]);
    assert_eq!(raw.tuning, [2.0, 0.5, 0.25 * ENV_DIFFUSE_WEIGHT, 4.0]);
    assert_eq!(raw.sky_irradiance[0], [0.25, 0.5, 0.75, 0.0]);
}

// `EnvironmentDiffuseScale` 0 turns off the sky bounce entirely; nothing else
// may leak it back in.
#[test]
fn a_zeroed_environment_scale_leaves_no_sky_bounce_at_all() {
    let lighting = Lighting {
        environment_diffuse: 0.0,
        ..Lighting::default()
    };

    let raw = unshadowed(&lighting, Vec3::ZERO);

    assert_eq!(raw.tuning[2], 0.0);
}

// Glare and Haze only exist on an `Atmosphere`; classic fog has to leave
// both at zero or the sky would grow a halo no place asked for.
#[test]
fn glare_and_haze_reach_the_shader_only_from_an_atmosphere() {
    let linear = unshadowed(&Lighting::default(), Vec3::ZERO);
    assert_eq!(linear.atmosphere_extra, [0.0; 4]);

    let lighting = Lighting {
        fog: Fog::Atmosphere {
            color: Vec3::ONE,
            decay: Vec3::ZERO,
            density: 0.2,
            offset: 0.0,
            glare: 0.2,
            haze: 1.14,
        },
        ..Lighting::default()
    };

    let atmosphere = unshadowed(&lighting, Vec3::ZERO);

    assert_eq!(atmosphere.atmosphere_extra, [0.2, 1.14, 0.0, 0.0]);
}

#[test]
fn the_sky_tint_carries_the_star_fade_in_its_fourth_slot() {
    let lighting = Lighting {
        sky_tint: Vec3::new(0.1, 0.2, 0.3),
        star_fade: 0.75,
        ..Lighting::default()
    };

    let raw = unshadowed(&lighting, Vec3::ZERO);

    assert_eq!(raw.sky_tint, [0.1, 0.2, 0.3, 0.75]);
}

/// The `name: type` members of one WGSL struct, in source order.
fn wgsl_members(shader: &str, name: &str) -> Vec<(String, String)> {
    let start = shader.find(&format!("struct {name} {{")).expect("declared");
    let block = &shader[start..start + shader[start..].find('}').expect("closed")];

    block
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .filter_map(|line| line.split_once(':'))
        .map(|(field, rest)| {
            (
                field.trim().to_string(),
                rest.trim().trim_end_matches(',').to_string(),
            )
        })
        .collect()
}

// Nothing but this checks the two declarations against each other: a field
// inserted on one side alone silently shifts every field after it, and the
// shader reads the wrong sixteen bytes with no error anywhere.
#[test]
fn the_uniform_and_the_shader_declare_the_same_fields_in_the_same_order() {
    let members = wgsl_members(include_str!("../lighting.wgsl"), "LightingUniform");
    let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();

    assert_eq!(
        names,
        [
            "sun_direction",
            "sun_color",
            "fill_color",
            "ambient",
            "fog_color",
            "fog_range",
            "atmosphere_color",
            "atmosphere_decay",
            "camera",
            "tuning",
            "sky_tint",
            "atmosphere_extra",
            "sky_irradiance",
            "light_view_projection",
            "shadow_params",
            "shadow_lamp",
            "locals",
            "quality",
            "clouds_color",
            "clouds_extra",
        ]
    );

    let bytes: usize = members
        .iter()
        .map(|(name, kind)| match kind.as_str() {
            "vec4<f32>" => 16,
            "mat4x4<f32>" => 64,
            "array<vec4<f32>, 6>" => 96,
            other => panic!("{name} has an unmeasured type {other}"),
        })
        .sum();
    assert_eq!(bytes as wgpu::BufferAddress, LightingRaw::SIZE);
}

// The shader picks which lamp to darken off this one marker, and does not
// sample the map at all when it is zero.
#[test]
fn the_lamp_marker_says_which_term_the_map_applies_to() {
    let fit = Fit::unfitted();

    for (lamp, marker) in [(Lamp::None, 0.0), (Lamp::Sun, 1.0), (Lamp::Fill, -1.0)] {
        let raw = LightingRaw::new(
            &Lighting::default(),
            Vec3::ZERO,
            &probe(),
            (lamp, &fit),
            0,
            &top(),
        );
        assert_eq!(raw.shadow_lamp[0], marker);
    }
}

// `ShadowSoftness` reaches the shader as a kernel radius in texels and
// nothing else; 0 has to land on the single tap that means a hard edge.
#[test]
fn shadow_softness_becomes_a_kernel_radius_in_texels() {
    let fit = Fit::unfitted();
    let radius = |softness| {
        let lighting = Lighting {
            shadow_softness: softness,
            ..Lighting::default()
        };
        LightingRaw::new(
            &lighting,
            Vec3::ZERO,
            &probe(),
            (Lamp::Sun, &fit),
            0,
            &top(),
        )
        .shadow_params[0]
    };

    assert_eq!(radius(0.0), 0.0);
    assert_eq!(radius(1.0), SOFTNESS_TEXELS);
    assert!(radius(0.2) > 1.0 && radius(0.2) < radius(1.0));
}

// The bias is quoted in studs and has to reach the shader in the [0, 1] the
// depth buffer stores, or a deep scene would be biased a hundred times too
// far and lift every shadow off its caster.
#[test]
fn the_receiver_bias_is_scaled_by_the_maps_own_depth_range() {
    let fit = Fit {
        depth_studs: 1200.0,
        texel_uv: 1.0 / 2048.0,
        ..Fit::unfitted()
    };

    let raw = LightingRaw::new(
        &Lighting::default(),
        Vec3::ZERO,
        &probe(),
        (Lamp::Sun, &fit),
        0,
        &top(),
    );

    assert_eq!(raw.shadow_params[3], DEPTH_BIAS_STUDS / 1200.0);
    // The kernel steps in texels of whatever map the level asked for, so this
    // comes from the fit rather than from a constant.
    assert_eq!(raw.shadow_params[1], 1.0 / 2048.0);
}

// Below quality level 7 the kernel collapses to the single tap that means a hard
// edge, however soft the place asked for its shadows to be.
#[test]
fn a_low_quality_level_hardens_the_shadow_edge() {
    let lighting = Lighting {
        shadow_softness: 1.0,
        ..Lighting::default()
    };
    let hard = QualityLevel::Level(5).profile();

    let raw = LightingRaw::new(
        &lighting,
        Vec3::ZERO,
        &probe(),
        (Lamp::Sun, &Fit::unfitted()),
        0,
        &hard,
    );

    assert_eq!(raw.shadow_params[0], 0.0);
}

// The shader loops `locals.x` times over a buffer that always holds at least one
// entry, so an unlit scene is told apart from a lit one by this count alone.
#[test]
fn the_local_light_count_reaches_the_shader_as_a_float() {
    let raw = LightingRaw::new(
        &Lighting::default(),
        Vec3::ZERO,
        &probe(),
        (Lamp::None, &Fit::unfitted()),
        7,
        &top(),
    );

    assert_eq!(raw.locals, [7.0, 0.0, 0.0, 0.0]);
    assert_eq!(unshadowed(&Lighting::default(), Vec3::ZERO).locals[0], 0.0);
}

// No `Clouds` at all has to reach the shader as `Cover = 0`, the same value a
// disabled or absent instance already collapses to on the CPU side (see
// `crate::lighting::clouds`) — one value for the sky shader's whole "draw
// nothing" case.
#[test]
fn no_clouds_packs_to_a_zeroed_cover() {
    let raw = unshadowed(&Lighting::default(), Vec3::ZERO);
    assert_eq!(raw.clouds_color, [0.0; 4]);
    assert_eq!(raw.clouds_extra, [0.0; 4]);
}

#[test]
fn clouds_cover_density_and_color_land_in_the_slots_the_shader_reads() {
    let lighting = Lighting {
        clouds: Some(Clouds {
            cover: 0.9,
            density: 0.65,
            color: Vec3::new(0.2, 0.3, 0.4),
        }),
        ..Lighting::default()
    };

    let raw = unshadowed(&lighting, Vec3::ZERO);

    assert_eq!(raw.clouds_color, [0.2, 0.3, 0.4, 0.0]);
    assert_eq!(raw.clouds_extra, [0.9, 0.65, 0.0, 0.0]);
}
