//! Where the sun stands in the sky for a given `Lighting`, and the other way
//! round: what `Lighting` has to say for the sun, or the moon, to stand
//! somewhere chosen.
//!
//! Both directions live here so the sky has one model. The renderer lights
//! with [`sun_direction`]; an editor that places the sun by pointing at the
//! scene asks [`place`] for the `ClockTime` and `GeographicLatitude` that put
//! it there, and gets back exactly what the renderer will then draw.

use glam::Vec3;

// The community reproduction of `Lighting:GetSunDirection()` (no official
// formula is published): Earth's axial tilt, the sky's 15 degrees an hour, and
// the 6 a.m. origin that puts sunrise on +X.
const AXIAL_TILT_DEGREES: f32 = 23.5;
const DEGREES_PER_HOUR: f32 = 15.0;
const SUNRISE_HOUR: f32 = 6.0;
const HOURS_PER_DAY: f32 = 24.0;
/// [`place`] never writes a latitude past a pole. creator-docs gives
/// `GeographicLatitude` no range and does not say whether Roblox clamps it;
/// a value inside ±90° means the same thing whichever it does, and one
/// outside it might not.
const MAX_LATITUDE_DEGREES: f32 = 90.0;

/// What is left of the sunlight after the air between it and the ground,
/// for a sun `height` (the y of [`sun_direction`]) above the horizon: white
/// at the zenith, a little dimmer as it sinks (0.033 per extra air mass, the
/// Kasten-Young approximation of how much air the light crosses), and
/// yellow in the last ten degrees, blue lost first.
///
/// Measured on the real client with the `tone_sun_*` fidelity scenes, a
/// grey ramp lit from 90, 60, 30, 15 and 5 degrees up: 0.91 of the zenith's
/// light at 15 degrees, still neutral, and (0.77, 0.77, 0.28) of it at 5.
pub(crate) fn transmittance(height: f32) -> Vec3 {
    const PER_AIR_MASS: f32 = 0.033;
    const HORIZON_BLUE: f32 = 0.25;
    let elevation = height.clamp(0.0, 1.0).asin().to_degrees();
    let air_mass = 1.0 / (height.max(0.0) + 0.50572 * (elevation + 6.07995).powf(-1.6364));
    let gray = (-PER_AIR_MASS * (air_mass - 1.0)).exp();
    let t = ((height - 0.05) / 0.15).clamp(0.0, 1.0);
    let blue = HORIZON_BLUE + (1.0 - HORIZON_BLUE) * t * t * (3.0 - 2.0 * t);
    Vec3::new(gray, gray, gray * blue)
}

/// Direction of the sun in Roblox's own axes, as the only known-good community
/// reproduction of `Lighting:GetSunDirection()` computes it, Roblox publishes
/// no formula, so this is reverse-engineered rather than authoritative.
///
/// Checks out at the three angles anyone can name from memory: noon at the
/// tropic is straight up, 6:00 is on the horizon due +X, 18:00 due -X.
pub(crate) fn sun_direction(clock: f32, latitude_degrees: f32) -> Vec3 {
    let time = clock.rem_euclid(HOURS_PER_DAY);
    let latitude = (latitude_degrees - AXIAL_TILT_DEGREES).to_radians();
    let longitude = ((time - SUNRISE_HOUR) * DEGREES_PER_HOUR).to_radians();

    Vec3::new(
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    )
    .normalize_or(Vec3::Y)
}

/// Which body [`place`] points at the scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Body {
    #[default]
    Sun,
    Moon,
}

/// The `Lighting` settings [`place`] arrived at, and where they really put
/// the body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// `Lighting.ClockTime`, in hours.
    pub clock_time: f32,
    /// `Lighting.GeographicLatitude`, in degrees.
    pub geographic_latitude: f32,
    /// Unit vector toward the body under these settings: the direction asked
    /// for, unless [`Placement::clamped`].
    pub direction: Vec3,
    /// The direction asked for is one no latitude within ±90° reaches, so
    /// [`Placement::direction`] is the nearest one that is.
    pub clamped: bool,
}

/// The inverse of [`sun_direction`]: the clock time and latitude that put
/// `body` in the direction `toward`.
///
/// [`sun_direction`] is `(cos φ cos λ, cos φ sin λ, sin φ)` with
/// `φ = latitude − 23.5°` and `λ = (clock − 6h)·15°/h`, so `φ = asin z` and
/// `λ = atan2(y, x)`. `φ` spans ±90°, which puts the latitude anywhere from
/// −66.5° to 113.5°. Past 90° there is no answer: that is a cone 23.5° wide
/// about +Z the sun never reaches, and a direction inside it is held at 90°,
/// the same hour, so the nearest point on the cone's rim.
///
/// The moon is placed through the sun it goes with: the real client's
/// `GetMoonDirection` is the sun's direction with X and Y turned round
/// (`crate::lighting::moon_direction`), which is its own inverse.
pub fn place(body: Body, toward: Vec3) -> Placement {
    let body_of = |sun: Vec3| match body {
        Body::Sun => sun,
        Body::Moon => crate::lighting::moon_direction(sun),
    };
    let sun = body_of(toward).normalize_or(Vec3::Y);
    let latitude = sun.z.clamp(-1.0, 1.0).asin().to_degrees() + AXIAL_TILT_DEGREES;
    let longitude = sun.y.atan2(sun.x).to_degrees();
    let clock_time = (longitude / DEGREES_PER_HOUR + SUNRISE_HOUR).rem_euclid(HOURS_PER_DAY);
    let geographic_latitude = latitude.min(MAX_LATITUDE_DEGREES);

    Placement {
        clock_time,
        geographic_latitude,
        direction: body_of(sun_direction(clock_time, geographic_latitude)),
        clamped: latitude > MAX_LATITUDE_DEGREES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < EPSILON
    }

    /// Every direction the sun can reach, on a grid of azimuth about +Z and
    /// elevation toward it, kept a hair short of the unreachable cone.
    fn reachable() -> impl Iterator<Item = Vec3> {
        let rim = (90.0 - AXIAL_TILT_DEGREES).to_radians();
        (0..36).flat_map(move |around| {
            (0..=20).map(move |up| {
                let azimuth = (around as f32 * 10.0).to_radians();
                let elevation = -std::f32::consts::FRAC_PI_2
                    + (rim + std::f32::consts::FRAC_PI_2 - 1e-3) * up as f32 / 20.0;
                Vec3::new(
                    elevation.cos() * azimuth.cos(),
                    elevation.cos() * azimuth.sin(),
                    elevation.sin(),
                )
            })
        })
    }

    #[test]
    fn placing_the_sun_and_reading_it_back_lands_where_it_was_asked() {
        for toward in reachable() {
            let placed = place(Body::Sun, toward);
            assert!(!placed.clamped, "{toward} is reachable");
            assert!(
                close(
                    sun_direction(placed.clock_time, placed.geographic_latitude),
                    toward
                ),
                "{toward} came back as {}",
                sun_direction(placed.clock_time, placed.geographic_latitude)
            );
            assert!(close(placed.direction, toward));
            assert!((-90.0..=90.0).contains(&placed.geographic_latitude));
        }
    }

    #[test]
    fn the_moon_is_placed_through_the_sun_it_goes_with() {
        for toward in reachable() {
            let moon = crate::lighting::moon_direction(toward);
            let placed = place(Body::Moon, moon);
            assert!(close(placed.direction, moon));
            assert!(close(
                sun_direction(placed.clock_time, placed.geographic_latitude),
                toward
            ));
        }
    }

    // The three settings the forward formula is checked against, read back.
    #[test]
    fn noon_sunrise_and_sunset_read_back_as_their_own_hours() {
        let noon = place(Body::Sun, Vec3::Y);
        assert!((noon.clock_time - 12.0).abs() < EPSILON);
        assert!((noon.geographic_latitude - AXIAL_TILT_DEGREES).abs() < EPSILON);
        assert!((place(Body::Sun, Vec3::X).clock_time - 6.0).abs() < EPSILON);
        assert!((place(Body::Sun, -Vec3::X).clock_time - 18.0).abs() < EPSILON);
    }

    // Inside the cone about +Z: held at the pole, on the same hour circle,
    // which is the rim's nearest point to where the cursor aimed.
    #[test]
    fn a_direction_past_the_pole_is_held_at_the_rim_and_says_so() {
        let rim = (90.0 - AXIAL_TILT_DEGREES).to_radians().sin();
        for toward in [
            Vec3::Z,
            Vec3::new(0.2, 0.1, 1.0).normalize(),
            Vec3::new(-0.3, -0.2, 1.0).normalize(),
        ] {
            let placed = place(Body::Sun, toward);
            assert!(placed.clamped, "{toward} is past the pole");
            assert_eq!(placed.geographic_latitude, MAX_LATITUDE_DEGREES);
            assert!((placed.direction.z - rim).abs() < EPSILON);
            let (asked, got) = (toward.truncate(), placed.direction.truncate());
            if asked.length() > EPSILON {
                assert!(asked.normalize().dot(got.normalize()) > 1.0 - EPSILON);
            }
        }
    }
}
