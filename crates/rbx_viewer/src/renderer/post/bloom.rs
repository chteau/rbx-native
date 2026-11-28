//! How `BloomEffect.Size` becomes a blur: the depth of the downsample chain and
//! the width of the tent each upsample spreads with.
//!
//! The chain is the usual one, halve until the wanted radius is a couple of
//! texels away, then walk back up adding each level into the one below it, so
//! "how wide is the blur" is answered by how many times it was halved, and the
//! remainder by the tent.

use crate::quality::NeonGlow;

/// Frame height `BloomEffect.Size` is quoted in pixels at.
///
/// Roblox's bloom is a screen-space effect whose Size is in pixels, so the same
/// place at half the resolution would glow twice as far across the image unless
/// the radius follows the frame. 1080p is the anchor because it is what Studio's
/// own viewport is on the reference captures.
pub(super) const REFERENCE_HEIGHT: f32 = 1080.0;

/// How deep the chain may go. Six halvings already reach a few texels on a 4K
/// frame, and every level past that costs a pass to blur almost nothing.
pub(super) const MAX_LEVELS: u32 = 6;

/// Tent radius, in texels of the level being sampled. Below half a texel the
/// nine taps all land inside one texel; above two they leave gaps the eye reads
/// as a grid.
const MIN_TENT: f32 = 0.5;
const MAX_TENT: f32 = 2.0;

/// The chain one `Size` asks for at one frame height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Blur {
    /// Levels in the chain, the first of which is half the frame.
    pub(super) levels: u32,
    /// Tent radius the upsamples spread with, in texels.
    pub(super) tent: f32,
}

/// The blur radius, in pixels of the frame being rendered.
pub(super) fn radius_pixels(size: f32, height: u32) -> f32 {
    size.max(0.0) * (height.max(1) as f32 / REFERENCE_HEIGHT)
}

/// How to reach that radius: one halving per doubling of it, with whatever is
/// left over folded into the tent.
///
/// The first level is already half the frame, so a chain of `n` levels spreads
/// about `2^n * tent` pixels of the full-resolution frame.
pub(super) fn blur(size: f32, height: u32) -> Blur {
    chain(radius_pixels(size, height))
}

/// `BloomEffect.Size` as a radius in pixels of the frame being rendered.
///
/// Not [`radius_pixels`]: Roblox's glow does not follow the frame. In
/// Studio 0.748 the same Neon parts in a 397 and a 591 pixel tall viewport
/// glow with about the same strength in pixels, and fade over 3.6 and 4.2
/// pixels: the reach grows as the 0.39 power of the height, and reads as
/// a radius of about 13.5 at 591 pixels.
pub(super) fn glow_radius_pixels(size: f32, height: u32) -> f32 {
    size.max(0.0) * GLOW_REACH * (height.max(1) as f32 / REFERENCE_HEIGHT).powf(GLOW_HEIGHT_POWER)
}

const GLOW_REACH: f32 = 0.71;
const GLOW_HEIGHT_POWER: f32 = 0.39;

/// How much wider Studio's reduced glow (levels 6 to 11) spreads than its
/// full one: Neon fading over 6.3 pixels rather than 4.4 in the same 591
/// pixel tall viewport.
const REDUCED_GLOW_REACH: f32 = 2.2;

/// [`blur`] for the bloom at a level drawing `neon` glow, whose radius is
/// [`glow_radius_pixels`].
pub(super) fn glow(size: f32, neon: NeonGlow, height: u32) -> Blur {
    let reach = if neon == NeonGlow::Reduced {
        REDUCED_GLOW_REACH
    } else {
        1.0
    };
    chain(glow_radius_pixels(size, height) * reach)
}

/// The depth the bloom's strength was fitted at: every level of the chain
/// adds the whole of what glows once more, so a deeper chain (a wider Size,
/// a taller frame, the reduced Neon glow) would glow brighter as well as
/// wider. Scaled back to this many, which is what the fitted frames (397 to
/// 720 pixels tall at Size 24) build.
const FITTED_LEVELS: f32 = 3.0;

/// What the bloom is multiplied by on a chain of `levels`.
pub(super) fn strength(levels: u32) -> f32 {
    FITTED_LEVELS / levels.max(1) as f32
}

fn chain(radius: f32) -> Blur {
    let radius = radius.max(1.0);
    // log2 of the radius in level-0 texels, which is what one halving buys.
    let halvings = (radius / 2.0).log2().round().clamp(1.0, MAX_LEVELS as f32);
    let levels = halvings as u32;

    Blur {
        levels,
        tent: (radius / (2.0 * halvings.exp2())).clamp(MIN_TENT, MAX_TENT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Size is quoted in pixels at 1080p, so the same place rendered twice as
    // tall has to glow twice as far or the two frames are different images.
    #[test]
    fn the_radius_follows_the_frame_height() {
        assert_eq!(radius_pixels(24.0, 1080), 24.0);
        assert_eq!(radius_pixels(24.0, 2160), 48.0);
        assert_eq!(radius_pixels(24.0, 540), 12.0);
        // And with Size, linearly: 56 for a place with high bloom against 24 for default.
        assert_eq!(radius_pixels(56.0, 1080), 56.0);
        assert!(radius_pixels(56.0, 626) > radius_pixels(24.0, 626));
    }

    #[test]
    fn a_negative_or_zero_size_asks_for_no_spread_at_all() {
        assert_eq!(radius_pixels(0.0, 1080), 0.0);
        assert_eq!(radius_pixels(-5.0, 1080), 0.0);
    }

    // Doubling the resolution has to land on the same *relative* blur: one more
    // halving, with the tent left where it was.
    #[test]
    fn the_chain_deepens_by_one_level_per_doubling_of_the_frame() {
        let half = blur(24.0, 540);
        let full = blur(24.0, 1080);
        let double = blur(24.0, 2160);

        assert_eq!(full.levels, half.levels + 1);
        assert_eq!(double.levels, full.levels + 1);
        assert!((full.tent - half.tent).abs() < 1e-6);
        assert!((double.tent - full.tent).abs() < 1e-6);
    }

    #[test]
    fn a_wider_size_asks_for_a_wider_blur() {
        let small = blur(24.0, 626);
        let large = blur(56.0, 626);

        assert!(large.levels >= small.levels);
        assert!(
            large.levels > small.levels || large.tent > small.tent,
            "{large:?} against {small:?}"
        );
    }

    // The chain has to stay usable at both ends: one level at least (there is
    // nothing to upsample from otherwise) and never past the cap.
    #[test]
    fn the_chain_is_bounded_at_both_ends() {
        for (size, height) in [(0.0, 480), (1.0, 240), (56.0, 4320), (1000.0, 2160)] {
            let blur = blur(size, height);
            assert!(
                (1..=MAX_LEVELS).contains(&blur.levels),
                "{size} at {height}: {blur:?}"
            );
            assert!((MIN_TENT..=MAX_TENT).contains(&blur.tent), "{blur:?}");
        }
    }

    // The glow's reach grows as the 0.39 power of the frame height, measured
    // in Studio at 397 and 591 pixels, and a radius of about 13.5 at 591.
    #[test]
    fn the_glow_follows_the_frame_far_less_than_the_frame_grows() {
        let at = |height| glow_radius_pixels(24.0, height);
        assert!((at(591) - 13.5).abs() < 0.2, "{}", at(591));
        assert!((at(397) / at(591) - (397.0f32 / 591.0).powf(0.39)).abs() < 1e-4);
        // Every frame Studio is used at builds the depth the strength was fitted on.
        for height in [397, 591, 720, 1080, 1440] {
            assert_eq!(glow(24.0, NeonGlow::Full, height).levels, 3, "{height}");
        }
        assert!(glow(24.0, NeonGlow::Reduced, 591).levels > 3);
    }

    // Each level adds what glows once more, so the strength comes back down
    // by the depth: a deeper chain only spreads the same glow wider.
    #[test]
    fn a_deeper_chain_glows_wider_not_brighter() {
        assert_eq!(strength(3), 1.0);
        assert!((strength(4) * 4.0 - strength(2) * 2.0).abs() < 1e-6);
    }

    // A 1080p frame at Studio's default Size: four halvings puts level 0 at
    // 540 rows and the last at about 34, which is the scale a 24-pixel spread
    // wants.
    #[test]
    fn studios_default_size_lands_on_four_levels() {
        assert_eq!(
            blur(24.0, 1080),
            Blur {
                levels: 4,
                tent: 0.75
            }
        );
    }
}
