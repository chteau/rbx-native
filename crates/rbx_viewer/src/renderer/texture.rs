//! Uploading a decoded image as a mipmapped GPU texture.

mod spread;

pub(super) use spread::{Pending, PER_FRAME};

use crate::assets::Image;
use crate::quality::MAX_TEXTURE_SIZE;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const CHANNELS: usize = 4;

/// Bind group layout shared by every textured pipeline: the image, then its
/// sampler.
pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview image"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Trilinear filtering, with as much anisotropy as the quality level allows.
///
/// Highly-tiled textures (e.g., a 2048-stud baseplate with 8-stud tiles = 256
/// repeats) become unreadable at grazing angles with trilinear filtering alone.
/// Anisotropic filtering samples along the surface's perspective direction to
/// preserve detail; `anisotropy` 1 turns it off, which is what the lowest
/// levels ask for.
pub(super) fn sampler(
    device: &wgpu::Device,
    address: wgpu::AddressMode,
    anisotropy: u16,
) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("rbxview image sampler"),
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: anisotropy.max(1),
        ..Default::default()
    })
}

/// An image uploaded once, whole mip chain and all.
///
/// Kept alive for the life of the renderer rather than dropped once bound: a
/// quality level lowering "apparent texture resolution" then only has to re-view
/// it from a lower mip, which costs no upload at all. See [`Uploaded::view`].
pub(super) struct Uploaded {
    texture: wgpu::Texture,
    levels: u32,
    /// Longest side of mip 0, which is the side a texture cap is measured on.
    side: u32,
}

impl Uploaded {
    /// Uploads `image` as colour.
    ///
    /// Everything binding a lone image paints with it, so `*Srgb` is the right
    /// view for all of them; only a `SurfaceAppearance`'s data maps ask for
    /// another format, through [`Uploaded::new`].
    pub(super) fn color(device: &wgpu::Device, queue: &wgpu::Queue, image: &Image) -> Self {
        Uploaded::new(device, queue, image, FORMAT)
    }

    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
        format: wgpu::TextureFormat,
    ) -> Self {
        let capped = capped(image, MAX_TEXTURE_SIZE);
        let image = &capped;
        let levels = mip_chain(image);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rbxview image"),
            size: extent(image.width, image.height),
            mip_level_count: u32::try_from(levels.len()).unwrap_or(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (level, mip) in levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: u32::try_from(level).unwrap_or(0),
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &mip.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(mip.width * CHANNELS as u32),
                    rows_per_image: Some(mip.height),
                },
                extent(mip.width, mip.height),
            );
        }

        Uploaded {
            texture,
            levels: u32::try_from(levels.len()).unwrap_or(1),
            side: image.width.max(image.height),
        }
    }

    /// The view a cap of `max_size` asks for: the very same texels, read from the
    /// first mip level small enough to respect it.
    pub(super) fn view(&self, max_size: u32) -> wgpu::TextureView {
        let base = skipped(self.side, self.levels, max_size);
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: base,
            mip_level_count: Some(self.levels - base),
            ..Default::default()
        })
    }

    pub(super) fn bind(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        max_size: u32,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview image"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.view(max_size)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

/// Downscales `image` until neither side exceeds `max_side`, halving (the same
/// box filter [`mip_chain`] itself uses) as many times as it takes.
///
/// No quality level ever asks to *view* a texture above
/// [`crate::quality::MAX_TEXTURE_SIZE`] — a level's own cap only moves the view
/// down the already-uploaded mip chain (see [`skipped`]) — so uploading
/// anything larger than that in the first place would just be VRAM no level
/// ever reads. An asset already at or under `max_side` is untouched.
fn capped(image: &Image, max_side: u32) -> Image {
    let mut current = image.clone();
    while current.width.max(current.height) > max_side {
        current = halve(&current);
    }
    current
}

/// Halves the image down to 1x1, box-filtering as it goes.
///
/// Shared with `envmap`, whose cube faces need the same chain: the mip levels
/// double as the environment probe's roughness prefilter.
///
/// The averaging happens on the stored sRGB bytes rather than on linearized
/// values: slightly too dark in theory, but it is what every offline mip
/// generator does, and matching them keeps textures looking like they do in
/// Studio.
pub(super) fn mip_chain(image: &Image) -> Vec<Image> {
    let mut levels = vec![image.clone()];
    while let Some(previous) = levels.last() {
        if previous.width <= 1 && previous.height <= 1 {
            break;
        }
        levels.push(halve(previous));
    }
    levels
}

/// How many mip levels a cap of `max_size` skips, `side` being the longest side
/// of level 0. This is how a quality level lowers "apparent texture resolution":
/// an already-uploaded image is read from a smaller mip instead of being
/// resampled or re-uploaded, so the switch costs nothing but the levels it skips.
///
/// Never the whole chain: the 1x1 level survives any cap, so no view is ever
/// left empty.
pub(super) fn skipped(side: u32, levels: u32, max_size: u32) -> u32 {
    let max_size = max_size.max(1);
    let mut skip = 0;
    while skip + 1 < levels && (side >> skip) > max_size {
        skip += 1;
    }
    skip
}

fn halve(image: &Image) -> Image {
    let width = (image.width / 2).max(1);
    let height = (image.height / 2).max(1);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * CHANNELS);

    for y in 0..height {
        for x in 0..width {
            // An odd source dimension leaves a final column or row with no
            // partner; clamping makes it average with itself instead of
            // sampling out of bounds.
            let x1 = (2 * x + 1).min(image.width - 1);
            let y1 = (2 * y + 1).min(image.height - 1);
            let corners = [(2 * x, 2 * y), (x1, 2 * y), (2 * x, y1), (x1, y1)];

            for channel in 0..CHANNELS {
                let sum: u32 = corners
                    .iter()
                    .map(|&(cx, cy)| u32::from(image.pixels[texel(image, cx, cy) + channel]))
                    .sum();
                pixels.push(u8::try_from(sum / 4).unwrap_or(u8::MAX));
            }
        }
    }

    Image {
        width,
        height,
        pixels,
    }
}

fn texel(image: &Image, x: u32, y: u32) -> usize {
    (y as usize * image.width as usize + x as usize) * CHANNELS
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, value: u8) -> Image {
        Image {
            width,
            height,
            pixels: vec![value; (width * height) as usize * CHANNELS],
        }
    }

    #[test]
    fn a_texture_past_the_cap_is_halved_down_to_it() {
        // 2048 halves once to 1024, which already fits a cap of 1024.
        let image = capped(&solid(2048, 2048, 9), 1024);
        assert_eq!((image.width, image.height), (1024, 1024));
    }

    #[test]
    fn a_texture_already_inside_the_cap_is_left_alone() {
        let image = solid(300, 200, 9);
        assert_eq!(capped(&image, 1024), image);
    }

    #[test]
    fn a_non_square_texture_is_capped_on_its_longest_side() {
        // 4096x1024 halves to 2048x512, then to 1024x256, which is the first
        // level whose longest side fits a cap of 1024.
        let image = capped(&solid(4096, 1024, 9), 1024);
        assert_eq!((image.width, image.height), (1024, 256));
    }

    #[test]
    fn the_chain_halves_down_to_a_single_texel() {
        let sizes: Vec<(u32, u32)> = mip_chain(&solid(8, 4, 0))
            .iter()
            .map(|level| (level.width, level.height))
            .collect();

        assert_eq!(sizes, vec![(8, 4), (4, 2), (2, 1), (1, 1)]);
    }

    #[test]
    fn a_one_by_one_image_has_no_smaller_level() {
        assert_eq!(mip_chain(&solid(1, 1, 7)).len(), 1);
    }

    #[test]
    fn an_odd_dimension_still_reaches_one_texel() {
        let sizes: Vec<(u32, u32)> = mip_chain(&solid(5, 3, 0))
            .iter()
            .map(|level| (level.width, level.height))
            .collect();

        assert_eq!(sizes, vec![(5, 3), (2, 1), (1, 1)]);
    }

    #[test]
    fn halving_averages_the_four_texels_it_replaces() {
        let image = Image {
            width: 2,
            height: 2,
            pixels: vec![
                0, 0, 0, 0, //
                100, 100, 100, 100, //
                100, 100, 100, 100, //
                200, 200, 200, 200,
            ],
        };

        assert_eq!(halve(&image).pixels, vec![100, 100, 100, 100]);
    }

    #[test]
    fn a_cap_starts_the_view_at_the_first_level_that_fits() {
        assert_eq!(skipped(8, 4, 2), 2);
        assert_eq!(skipped(8, 4, 8), 0);
    }

    #[test]
    fn a_cap_above_the_image_skips_nothing() {
        assert_eq!(skipped(4, 3, 1024), 0);
    }

    // A non-square image is capped on its longest side, since that is the one a
    // texture memory budget is set by.
    #[test]
    fn a_wide_image_is_capped_on_its_longest_side() {
        // 8x2 halves to 4x1, which is the first level inside a cap of 4.
        assert_eq!(skipped(8, 4, 4), 1);
    }

    // A cap far below the image must still leave one level to sample.
    #[test]
    fn no_cap_empties_the_chain() {
        assert_eq!(skipped(1024, 11, 0), 10);
        assert_eq!(skipped(1, 1, 1024), 0);
    }

    #[test]
    fn every_level_is_tightly_packed_rgba() {
        for level in mip_chain(&solid(6, 6, 42)) {
            assert_eq!(
                level.pixels.len(),
                (level.width * level.height) as usize * CHANNELS
            );
            assert!(level.pixels.iter().all(|&byte| byte == 42));
        }
    }
}
