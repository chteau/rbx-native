//! The star field `Sky.StarCount` asks for: a fixed set of directions on the
//! unit sphere, with a brightness each.
//!
//! Roblox's own stars are a fixed field too — they do not move between runs of
//! the same place — so these are generated from a constant seed rather than
//! from the clock, and the same count always yields the same sky.

use glam::Vec3;

/// Studio's default `StarCount`, and what a `Sky` that never set it gets.
pub(crate) const DEFAULT_COUNT: u32 = 3000;

/// Ceiling on the field, which Studio does not impose: a place asking for
/// millions of stars would otherwise spend a hundred megabytes of vertices on
/// specks smaller than a pixel.
pub(crate) const MAX_COUNT: u32 = 20_000;

/// The fractional golden ratio, which is what `splitmix64` mixes each step by.
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
/// Any constant would do, as long as it never changes: the field has to be the
/// same on every run.
const SEED: u64 = 0x5DEE_CE66_D000_0001;

/// Dimmest a star draws, as a fraction of full brightness. The cube below puts
/// most of the field near this floor and leaves a handful bright, which is what
/// a real sky looks like and what keeps the field from reading as noise.
const MIN_MAGNITUDE: f32 = 0.2;

/// One star: where on the sky it sits, and how brightly it draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Star {
    pub(crate) direction: Vec3,
    pub(crate) magnitude: f32,
}

/// `count` stars spread evenly over the whole sphere.
///
/// Evenly in the sense that matters here: the z of a uniform point on a sphere
/// is itself uniform, so taking it straight from the generator avoids the
/// crowding at the poles that picking two angles would give.
pub(crate) fn field(count: u32) -> Vec<Star> {
    let mut state = SEED;
    let mut next = move || {
        state = state.wrapping_add(GOLDEN_GAMMA);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // The top 24 bits are the best mixed, and 24 is all an f32 holds.
        (z >> 40) as f32 / (1u32 << 24) as f32
    };

    (0..count.min(MAX_COUNT))
        .map(|_| {
            let height = next().mul_add(2.0, -1.0);
            let radius = (1.0 - height * height).max(0.0).sqrt();
            let angle = next() * std::f32::consts::TAU;
            let brightness = next();

            Star {
                // Y is up, so the height goes there and the other two spread
                // round the horizon.
                direction: Vec3::new(radius * angle.cos(), height, radius * angle.sin())
                    .normalize_or(Vec3::Y),
                magnitude: MIN_MAGNITUDE
                    + (1.0 - MIN_MAGNITUDE) * brightness * brightness * brightness,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_holds_exactly_the_stars_it_was_asked_for() {
        assert_eq!(field(0).len(), 0);
        assert_eq!(field(1).len(), 1);
        assert_eq!(field(DEFAULT_COUNT).len(), 3000);
        // A place asking for more than the cap gets the cap, not a refusal.
        assert_eq!(field(u32::MAX).len(), MAX_COUNT as usize);
    }

    // A direction that is not a unit vector would put its star off the sphere,
    // where the quad it is built into would be the wrong size.
    #[test]
    fn every_star_sits_on_the_unit_sphere() {
        for star in field(2000) {
            assert!((star.direction.length() - 1.0).abs() < 1e-4, "{star:?}");
            assert!((MIN_MAGNITUDE..=1.0).contains(&star.magnitude), "{star:?}");
        }
    }

    // The field is meant to be part of the place, not of the run: two viewers
    // of the same scene have to see the same stars.
    #[test]
    fn the_same_count_always_gives_the_same_field() {
        assert_eq!(field(500), field(500));
        // And a longer field extends the shorter one rather than reshuffling it.
        assert_eq!(field(500)[..100], field(100)[..]);
    }

    // Spread, not clustered: a generator that crowded the poles (or, worse, one
    // axis) would draw a band rather than a sky.
    #[test]
    fn the_field_covers_the_whole_sphere() {
        let stars = field(4000);
        let above = stars.iter().filter(|star| star.direction.y > 0.0).count();
        assert!((1800..2200).contains(&above), "{above} above the horizon");

        // The cap past half an axis is a quarter of the sphere whichever axis
        // it is taken on, so a generator favouring one of them shows up here.
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                let share = stars
                    .iter()
                    .filter(|star| star.direction.to_array()[axis] * sign > 0.5)
                    .count();
                assert!((850..1150).contains(&share), "axis {axis} {sign}: {share}");
            }
        }
    }
}
