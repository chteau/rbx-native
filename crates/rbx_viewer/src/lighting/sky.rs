//! How the sky follows the sun below the horizon: what the skybox (and the
//! light it bounces) is multiplied by, and when the stars come out.
//!
//! Roblox keeps one set of skybox images for the whole day and darkens them as
//! the sun sets — a place with a noon panorama still reads as night at
//! `ClockTime = 0`. These two curves are that darkening, fitted by eye against
//! Studio rather than to any published formula.

use glam::Vec3;

/// Sun elevation, as `sun_direction.y`, above which the sky keeps its full
/// daylight brightness: about ten degrees up, where Studio's own sky has
/// finished brightening.
const DAY_ELEVATION: f32 = 0.17;
/// And below which it is fully night. Well under the horizon rather than at it:
/// Roblox keeps a lit sky for a good while after sunset, and `ClockTime 18.4` —
/// six degrees past it — is still plainly dusk in Studio rather than midnight.
const NIGHT_ELEVATION: f32 = -0.25;

/// What is left of the sky at midnight. Not zero: a Roblox night sky is dim but
/// plainly visible, and its clouds still read against the stars.
const NIGHT_BRIGHTNESS: f32 = 0.08;
/// And the cast it takes on the way there. Moonlight is the same blue the fill
/// lamp turns at night (see the parent module), which is what keeps a night sky
/// from reading as a grey daytime one behind a neutral-density filter.
const NIGHT_TINT: Vec3 = Vec3::new(0.55, 0.7, 1.0);

/// Stars are out entirely below this elevation and gone by the time the sun is
/// back on the horizon, which is what "they come out as the sun sets" means in
/// a renderer with no twilight scattering of its own.
const STARS_FULL_ELEVATION: f32 = -0.25;
const STARS_GONE_ELEVATION: f32 = 0.0;

/// What the skybox, the sky's irradiance and the haze are all multiplied by at
/// this sun elevation: white in daylight, a dim blue at night.
pub(super) fn tint(sun_elevation: f32) -> Vec3 {
    let day = smoothstep(NIGHT_ELEVATION, DAY_ELEVATION, sun_elevation);

    (NIGHT_TINT * NIGHT_BRIGHTNESS).lerp(Vec3::ONE, day)
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
    // sky exactly as its panels were painted, and midnight lands on the dim blue
    // this module is for.
    #[test]
    fn the_sky_is_untouched_by_day_and_dim_blue_at_night() {
        assert_eq!(tint(1.0), Vec3::ONE);
        assert_eq!(tint(DAY_ELEVATION), Vec3::ONE);

        let night = tint(-1.0);
        assert_eq!(night, NIGHT_TINT * NIGHT_BRIGHTNESS);
        assert_eq!(night, tint(NIGHT_ELEVATION));
        // Blue survives the dimming better than red, or a night sky would just
        // be a grey one turned down.
        assert!(night.z > night.x);
        assert!(night.z <= NIGHT_BRIGHTNESS);
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
