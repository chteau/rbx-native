use glam::Vec3;

use super::*;
use crate::lighting::Fog;
use crate::quality::QualityLevel;
use crate::scene::linear_to_srgb;

fn probe() -> Probe {
    Probe {
        top_mip: 7.0,
        // A strongly blue sky, so a neutral one cannot be mistaken for it.
        irradiance: [[0.1, 0.2, 0.9, 0.0]; FACES],
    }
}

/// A place lit as unlike the neutral key as a place can be: night, a thick
/// red atmosphere, shadows off, a dark exposure.
fn gloomy() -> Lighting {
    Lighting {
        sun_direction: Vec3::new(0.0, -1.0, 0.0),
        sun_color: Vec3::ZERO,
        ambient: Vec3::new(0.3, 0.0, 0.0),
        exposure: 0.25,
        global_shadows: false,
        fog: Fog::Atmosphere {
            color: Vec3::new(1.0, 0.0, 0.0),
            decay: Vec3::ONE,
            density: 1.0,
            offset: 0.0,
            glare: 1.0,
            haze: 3.0,
        },
        ..Lighting::default()
    }
}

#[test]
fn an_isolated_frame_draws_none_of_the_places_surroundings_but_keeps_the_effects_pass() {
    assert_eq!(
        Surroundings::of(true),
        Surroundings {
            sky: false,
            terrain: false,
            effects: true,
            adornments: false,
            screen_gui: false,
            local_lights: false,
        }
    );
}

#[test]
fn a_normal_frame_draws_every_one_of_them() {
    assert_eq!(
        Surroundings::of(false),
        Surroundings {
            sky: true,
            terrain: true,
            effects: true,
            adornments: true,
            screen_gui: true,
            local_lights: true,
        }
    );
}

// Linearised for the HDR target, the backdrop has to come back out of the
// `*Srgb` encode as the very grey the constant names.
#[test]
fn the_backdrop_encodes_back_to_its_own_grey() {
    let color = clear_color(true, wgpu::Color::BLACK);
    for channel in [color.r, color.g, color.b] {
        let encoded = (linear_to_srgb(channel as f32) * 255.0).round() as u8;
        assert_eq!(encoded, ISOLATION_BACKDROP[0]);
    }
    assert_eq!(clear_color(false, wgpu::Color::RED), wgpu::Color::RED);
}

#[test]
fn an_isolated_frame_is_lit_the_same_whatever_the_place_says() {
    let quality = QualityLevel::default().profile();
    let (from_gloom, gloom_probe, gloom_quality) = lit(true, &gloomy(), probe(), quality);
    let (from_default, ..) = lit(true, &Lighting::default(), probe(), quality);

    assert_eq!(from_gloom, from_default);
    assert_eq!(from_gloom, Lighting::neutral());
    // The key is up and in front, it casts, and nothing fogs the rig.
    assert!(from_gloom.sun_direction.y > 0.5 && from_gloom.sun_direction.z < 0.0);
    assert!(from_gloom.global_shadows);
    assert_eq!(from_gloom.exposure, 1.0);
    assert!(matches!(from_gloom.fog, Fog::Linear { start, .. } if start >= 1.0e6));
    assert!(from_gloom.clouds.is_none());
    assert_eq!(from_gloom.effects, crate::lighting::Effects::default());
    // A grey sky all round, read flat rather than off the place's cube.
    for face in gloom_probe.irradiance {
        assert_eq!(face[0], face[1]);
        assert_eq!(face[1], face[2]);
    }
    assert_eq!(gloom_probe.top_mip, probe().top_mip);
    assert!(!gloom_quality.env_reflections);
}

#[test]
fn a_normal_frame_is_lit_by_the_place_itself() {
    let quality = QualityLevel::default().profile();
    let (lighting, kept, kept_quality) = lit(false, &gloomy(), probe(), quality);

    assert_eq!(lighting, gloomy());
    assert_eq!(kept, probe());
    assert_eq!(kept_quality, quality);
}
