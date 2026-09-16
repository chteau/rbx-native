//! The material texture packs on the GPU: one `texture_2d_array` per map kind,
//! one layer per material the scene uses.
//!
//! Bind group 1 of every surface pipeline (see `renderer::pipeline`), so a part
//! and a file mesh sample their material exactly the same way.

use rbx_materials::MapKind;

use super::texture;
use crate::assets::Image;
use crate::quality::QualityProfile;
use crate::scene::{Catalog, Maps};

/// Every pack Roblox publishes is 1024²; anything else (a `MaterialVariant`'s
/// own maps) is resampled to it, since one array holds one size for all layers.
/// The arrays are always built at this size whatever the quality level asks for:
/// a level that lowers texture resolution only moves the *view* down the mip
/// chain (see [`Materials::set_quality`]), so switching level re-uploads nothing.
const RESOLUTION: u32 = 1024;
const CHANNELS: usize = 4;

/// What a layer gets where its material has no such map: white leaves the
/// part's own colour alone, the flat normal points straight out, and a fully
/// rough dielectric is the least eventful surface there is.
fn neutral(kind: MapKind) -> [u8; CHANNELS] {
    match kind {
        MapKind::Color => [255, 255, 255, 255],
        MapKind::Normal => [128, 128, 255, 255],
        MapKind::Roughness => [230, 230, 230, 255],
        MapKind::Metalness => [0, 0, 0, 255],
    }
}

/// Only the colour map holds colour; the other three are data, and reading them
/// through an sRGB view would bend every value they carry.
fn format(kind: MapKind) -> wgpu::TextureFormat {
    match kind {
        MapKind::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        _ => wgpu::TextureFormat::Rgba8Unorm,
    }
}

pub(super) struct Materials {
    pub(super) bind_group: wgpu::BindGroup,
    /// One array per map kind, in [`MapKind::ALL`] order, kept alive so a change
    /// of quality level only has to re-view and re-bind them.
    arrays: Vec<wgpu::Texture>,
    /// What the arrays were uploaded from (see [`Catalog::resolved_maps`]), so
    /// a scene rebuild can tell whether the new scene's catalog would upload
    /// the very same texels — the common case for any edit that is not a new
    /// material — and keep them (see [`Materials::holds`]).
    maps: Vec<Maps>,
}

pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let array = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    };

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview materials"),
        entries: &[
            array(0),
            array(1),
            array(2),
            array(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

impl Materials {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        catalog: &Catalog,
        quality: &QualityProfile,
    ) -> Self {
        let arrays: Vec<wgpu::Texture> = MapKind::ALL
            .iter()
            .map(|&kind| array(device, queue, catalog, kind))
            .collect();

        Materials {
            bind_group: bind(device, layout, &arrays, quality),
            arrays,
            maps: catalog.resolved_maps(),
        }
    }

    /// Whether the arrays already hold exactly what `catalog` would upload —
    /// every layer, every map, in the same order, so every instance buffer's
    /// layer index still points at the right texels. This is the whole of the
    /// arrays' identity: their size is fixed (see [`RESOLUTION`]) and the
    /// quality level only re-views them.
    pub(super) fn holds(&self, catalog: &Catalog) -> bool {
        self.maps == catalog.resolved_maps()
    }

    /// Re-views the packs at the new texture cap and rebuilds the sampler at the
    /// new anisotropy. No layer is resampled and nothing is uploaded: the cap is
    /// the mip level the array is read from.
    pub(super) fn set_quality(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        quality: &QualityProfile,
    ) {
        self.bind_group = bind(device, layout, &self.arrays, quality);
    }
}

/// Views every array from the level the cap allows and binds the lot behind one
/// sampler.
fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    arrays: &[wgpu::Texture],
    quality: &QualityProfile,
) -> wgpu::BindGroup {
    // Repeat, not clamp: a material is a tiling pattern, and the whole point
    // of studs-per-tile is that the UVs run well past 1.
    let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);
    let levels = mip_levels(RESOLUTION);
    let base = texture::skipped(RESOLUTION, levels, quality.texture_max_size);
    let views: Vec<wgpu::TextureView> = arrays
        .iter()
        .map(|array| {
            array.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_mip_level: base,
                mip_level_count: Some(levels - base),
                ..Default::default()
            })
        })
        .collect();

    let entries: Vec<wgpu::BindGroupEntry<'_>> = views
        .iter()
        .enumerate()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .chain(std::iter::once(wgpu::BindGroupEntry {
            binding: MapKind::ALL.len() as u32,
            resource: wgpu::BindingResource::Sampler(&sampler),
        }))
        .collect();

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview materials"),
        layout,
        entries: &entries,
    })
}

/// Uploads one map kind for every layer, with its whole mip chain.
fn array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    catalog: &Catalog,
    kind: MapKind,
) -> wgpu::Texture {
    let layers = u32::try_from(catalog.layers()).unwrap_or(1).max(1);
    let levels = mip_levels(RESOLUTION);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview material array"),
        size: extent(layers, RESOLUTION),
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: format(kind),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Built once and shared by every layer that lacks this map, which is most
    // of them for metalness.
    let mut blank: Option<Vec<Image>> = None;
    for layer in 0..layers {
        let chain = match catalog.image(layer as usize, kind) {
            Some(image) => texture::mip_chain(&fit(image, RESOLUTION)),
            None => blank
                .get_or_insert_with(|| texture::mip_chain(&solid(neutral(kind), RESOLUTION)))
                .clone(),
        };
        write(queue, &texture, layer, &chain);
    }

    texture
}

fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, layer: u32, chain: &[Image]) {
    for (level, mip) in chain.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: u32::try_from(level).unwrap_or(0),
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &mip.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(mip.width * CHANNELS as u32),
                rows_per_image: Some(mip.height),
            },
            wgpu::Extent3d {
                width: mip.width,
                height: mip.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn extent(layers: u32, resolution: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: resolution,
        height: resolution,
        depth_or_array_layers: layers,
    }
}

fn mip_levels(resolution: u32) -> u32 {
    resolution.ilog2() + 1
}

fn solid(color: [u8; CHANNELS], resolution: u32) -> Image {
    Image {
        width: resolution,
        height: resolution,
        pixels: color
            .iter()
            .copied()
            .cycle()
            .take((resolution * resolution) as usize * CHANNELS)
            .collect(),
    }
}

/// Nearest-neighbour resampling to the array's own size.
///
/// Only a hand-authored `MaterialVariant` map — or a quality level that caps the
/// array below the packs' own 1024 — ever needs it: a tiling pattern rescaled by
/// a factor near 1 loses nothing a box filter would have saved.
fn fit(image: &Image, resolution: u32) -> Image {
    if image.width == resolution && image.height == resolution {
        return image.clone();
    }

    let mut pixels = Vec::with_capacity((resolution * resolution) as usize * CHANNELS);
    for y in 0..resolution {
        let source_y = (y * image.height / resolution).min(image.height.saturating_sub(1));
        for x in 0..resolution {
            let source_x = (x * image.width / resolution).min(image.width.saturating_sub(1));
            let start = (source_y as usize * image.width as usize + source_x as usize) * CHANNELS;
            match image.pixels.get(start..start + CHANNELS) {
                Some(texel) => pixels.extend_from_slice(texel),
                None => pixels.extend_from_slice(&[0; CHANNELS]),
            }
        }
    }

    Image {
        width: resolution,
        height: resolution,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::quality::QualityLevel;

    #[test]
    fn the_array_carries_a_full_mip_chain() {
        assert_eq!(mip_levels(RESOLUTION), 11);
        assert_eq!(texture::mip_chain(&solid([1; 4], RESOLUTION)).len(), 11);
    }

    // The arrays are always 1024²; a capped level reads them from a lower mip
    // instead, which is what makes changing level free of any upload.
    #[test]
    fn a_capped_level_only_moves_the_view_down_the_chain() {
        let base = |level: u8| {
            texture::skipped(
                RESOLUTION,
                mip_levels(RESOLUTION),
                QualityLevel::Level(level).profile().texture_max_size,
            )
        };

        assert_eq!(base(1), 2);
        assert_eq!(base(3), 1);
        assert_eq!(base(QualityLevel::MAX), 0);
    }

    #[test]
    fn an_image_of_another_size_is_resampled_to_the_array() {
        let small = Image {
            width: 2,
            height: 2,
            pixels: vec![
                1, 1, 1, 255, //
                2, 2, 2, 255, //
                3, 3, 3, 255, //
                4, 4, 4, 255,
            ],
        };
        let fitted = fit(&small, RESOLUTION);

        assert_eq!(fitted.width, RESOLUTION);
        assert_eq!(fitted.height, RESOLUTION);
        // Each source texel covers a quarter of the result.
        assert_eq!(fitted.pixels[0], 1);
        assert_eq!(fitted.pixels[(RESOLUTION as usize - 1) * CHANNELS], 2);
        assert_eq!(
            fitted.pixels[(RESOLUTION as usize * RESOLUTION as usize - 1) * CHANNELS],
            4
        );
    }

    #[test]
    fn an_image_already_the_right_size_is_left_alone() {
        let exact = solid([7, 8, 9, 255], RESOLUTION);
        assert_eq!(fit(&exact, RESOLUTION), exact);
    }

    #[test]
    fn a_neutral_layer_changes_nothing_it_multiplies() {
        assert_eq!(neutral(MapKind::Color), [255, 255, 255, 255]);
        assert_eq!(neutral(MapKind::Metalness)[0], 0);
        // Flat normal: (0, 0, 1) once decoded from the 0-1 range.
        assert_eq!(neutral(MapKind::Normal), [128, 128, 255, 255]);
    }
}
