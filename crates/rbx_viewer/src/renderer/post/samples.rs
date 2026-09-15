//! How many samples the HDR frame is actually drawn at, once the adapter's own
//! guarantees are applied to what the quality level asked for.

use super::super::pipeline::DEPTH_FORMAT;
use super::HDR_FORMAT;

/// The sample count this device can actually draw the HDR frame at, from what the
/// level asked for downwards.
///
/// `Rgba16Float` and `Depth32Float` are both guaranteed 4x by the WebGPU spec, and
/// the colour format is guaranteed resolvable with it, so in practice this returns
/// what it was given; it exists so a backend that guarantees less degrades to a
/// plain single-sampled frame instead of failing to create a pipeline.
pub(super) fn supported(device: &wgpu::Device, wanted: u32) -> u32 {
    let flags =
        |format: wgpu::TextureFormat| format.guaranteed_format_features(device.features()).flags;

    highest(flags(HDR_FORMAT), flags(DEPTH_FORMAT), wanted)
}

fn highest(
    color: wgpu::TextureFormatFeatureFlags,
    depth: wgpu::TextureFormatFeatureFlags,
    wanted: u32,
) -> u32 {
    if !color.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE) {
        return 1;
    }

    [4, 2, 1]
        .into_iter()
        .find(|&count| {
            count <= wanted
                && color.sample_count_supported(count)
                && depth.sample_count_supported(count)
        })
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESOLVABLE_X4: wgpu::TextureFormatFeatureFlags =
        wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4
            .union(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE);

    #[test]
    fn the_top_band_gets_the_four_samples_it_asks_for() {
        assert_eq!(highest(RESOLVABLE_X4, RESOLVABLE_X4, 4), 4);
        assert_eq!(highest(RESOLVABLE_X4, RESOLVABLE_X4, 1), 1);
    }

    // A colour format that cannot be resolved is no use multisampled: the whole
    // post chain samples the resolved texture.
    #[test]
    fn an_unresolvable_colour_format_falls_back_to_one_sample() {
        assert_eq!(
            highest(
                wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4,
                RESOLVABLE_X4,
                4
            ),
            1
        );
    }

    #[test]
    fn a_depth_format_stuck_at_one_sample_holds_the_colour_back() {
        assert_eq!(
            highest(RESOLVABLE_X4, wgpu::TextureFormatFeatureFlags::empty(), 4),
            1
        );
    }
}
