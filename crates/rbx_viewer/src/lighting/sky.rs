//! How the sky follows the sun below the horizon: what the skybox (and the
//! light it bounces) is multiplied by, and when the stars come out.
//!
//! Roblox keeps one set of skybox images for the whole day and darkens them as
//! the sun sets, a place with a noon panorama still reads as night at
//! `ClockTime = 0`. The darkening is measured on the real client (the sky
//! behind the `tone_sun_*` fidelity scenes and the `evening` and `night` ones):
//! untouched down to 15 degrees, half as bright at 5, a third at 3 and a
//! thirtieth at midnight, and not blue, a shade warm if anything.

use glam::Vec3;

/// Sun elevation, as `sun_direction.y`, above which the sky keeps its full
/// daylight brightness: 15 degrees up.
const DAY_ELEVATION: f32 = 0.26;
/// And below which it is fully night, about six degrees under the horizon.
const NIGHT_ELEVATION: f32 = -0.1;

/// What is left of the sky at midnight: (0.029, 0.02, 0.02) of its noon
/// light on the real client.
const NIGHT_BRIGHTNESS: f32 = 0.029;
const NIGHT_TINT: Vec3 = Vec3::new(1.0, 0.72, 0.7);

/// What the sky is multiplied by with the sun a few degrees up: the real
/// client's sky (and the light it bounces) at `ClockTime` 17.8 on the equator
/// over ours, in linear light (the `evening` fidelity scene). It fades out by
/// [`WARM_GONE_ELEVATION`] (about 13 degrees, the `morning` scene is
/// untouched) and toward night, which has its own [`NIGHT_TINT`].
const TWILIGHT_TINT: Vec3 = Vec3::new(1.12, 0.8, 0.6);
const WARM_FULL_ELEVATION: f32 = 0.04;
const WARM_GONE_ELEVATION: f32 = 0.22;

/// Stars are out entirely below this elevation and gone by the time the sun is
/// back on the horizon, which is what "they come out as the sun sets" means in
/// a renderer with no twilight scattering of its own.
const STARS_FULL_ELEVATION: f32 = -0.25;
const STARS_GONE_ELEVATION: f32 = 0.0;

/// What the skybox, the sky's irradiance and the haze are all multiplied by at
/// this sun elevation: white in daylight, nearly black at night.
pub(super) fn tint(sun_elevation: f32) -> Vec3 {
    let day = smoothstep(NIGHT_ELEVATION, DAY_ELEVATION, sun_elevation);

    let warmth = (1.0 - smoothstep(WARM_FULL_ELEVATION, WARM_GONE_ELEVATION, sun_elevation))
        * smoothstep(NIGHT_ELEVATION, 0.0, sun_elevation);

    (NIGHT_TINT * NIGHT_BRIGHTNESS).lerp(Vec3::ONE, day) * Vec3::ONE.lerp(TWILIGHT_TINT, warmth)
}

/// How much of the star field shows, 0 by day to 1 at night.
pub(super) fn star_fade(sun_elevation: f32) -> f32 {
    1.0 - smoothstep(STARS_FULL_ELEVATION, STARS_GONE_ELEVATION, sun_elevation)
}

/// Hermite interpolation, as every shading language spells it: `glam` has no
/// scalar equivalent.
fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);

    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The two ends of the curve are the whole contract: full daylight leaves the
    // sky exactly as its panels were painted, and midnight lands on the dim,
    // faintly warm sky the real client shows.
    #[test]
    fn the_sky_is_untouched_by_day_and_nearly_black_at_night() {
        assert_eq!(tint(1.0), Vec3::ONE);
        assert_eq!(tint(DAY_ELEVATION), Vec3::ONE);

        let night = tint(-1.0);
        assert_eq!(night, NIGHT_TINT * NIGHT_BRIGHTNESS);
        assert_eq!(night, tint(NIGHT_ELEVATION));
        assert!(night.x >= night.z);
        assert!(night.max_element() <= NIGHT_BRIGHTNESS);
    }

    #[test]
    fn a_low_sun_warms_the_sky() {
        let dusk = tint(0.05);
        assert!(dusk.x > dusk.y && dusk.y > dusk.z, "{dusk}");
        assert_eq!(tint(WARM_GONE_ELEVATION), Vec3::ONE);
    }

    #[test]
    fn the_sky_darkens_monotonically_as_the_sun_goes_down() {
        let mut previous = tint(-1.0).z;
        for step in 0..=20u8 {
            let elevation = f32::from(step).mul_add(0.1, -1.0);
            let brightness = tint(elevation).z;
            assert!(brightness >= previous - 1e-6, "{elevation}");
            previous = brightness;
        }
    }

    #[test]
    fn no_star_shows_while_the_sun_is_up() {
        assert_eq!(star_fade(1.0), 0.0);
        assert_eq!(star_fade(STARS_GONE_ELEVATION), 0.0);
        assert_eq!(star_fade(STARS_FULL_ELEVATION), 1.0);
        assert_eq!(star_fade(-1.0), 1.0);
        // Dusk, halfway through the band: visible, but not yet the full field.
        let dusk = star_fade((STARS_FULL_ELEVATION + STARS_GONE_ELEVATION) / 2.0);
        assert!(dusk > 0.0 && dusk < 1.0, "{dusk}");
    }
}
