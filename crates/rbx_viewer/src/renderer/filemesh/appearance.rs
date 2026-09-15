//! One bind group per `SurfaceAppearance`: its four maps as ordinary 2D
//! textures sampled with the mesh's own UVs, plus the small uniform carrying the
//! set's tint and alpha mode.
//!
//! Unlike `renderer::material`, nothing here is resampled into an array: a
//! `SurfaceAppearance`'s maps are authored for one mesh and come in whatever
//! size their author chose.

use bytemuck::{Pod, Zeroable};
use rbx_materials::MapKind;
use wgpu::util::DeviceExt;

use super::super::texture;
use super::images::Binding;
use crate::assets::Image;
use crate::scene::{AlphaMode, Appearance, Resolved};

/// What a map kind contributes where the set does not carry it. White at zero
/// alpha is the colour map's neutral, which under `Overlay` — the mode a set
/// without a colour map is forced into — leaves the part's own colour untouched.
fn neutral(kind: MapKind) -> [u8; 4] {
    match kind {
        MapKind::Color => [255, 255, 255, 0],
        MapKind::Normal => [128, 128, 255, 255],
        MapKind::Metalness => [0, 0, 0, 255],
        MapKind::Roughness => [230, 230, 230, 255],
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

/// The per-set constants, as bind group 2's uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniform {
    /// xyz: `SurfaceAppearance.Color`, linear. w: 1 for `AlphaMode.Transparency`
    /// and 0 for `Overlay`, which is the only thing that tells the fragment
    /// shader whether the colour map's alpha blends or reveals the part colour.
    tint: [f32; 4],
}

pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let map = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview surface appearance"),
        entries: &[
            map(0),
            map(1),
            map(2),
            map(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

/// Every `SurfaceAppearance` the scene resolved, uploaded once and bound in the
/// order their indices address them.
pub(super) struct Sets {
    sets: Vec<Set>,
    pub(super) bind_groups: Vec<wgpu::BindGroup>,
}

/// One set's four maps and the constants beside them, kept alive so a change of
/// quality level rebuilds the bind group and nothing else.
struct Set {
    maps: Vec<texture::Uploaded>,
    uniform: wgpu::Buffer,
}

impl Sets {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding: Binding<'_>,
        resolved: &Resolved,
    ) -> Self {
        let sets: Vec<Set> = resolved
            .appearances
            .iter()
            .map(|appearance| upload(device, queue, appearance, resolved))
            .collect();
        let bind_groups = sets.iter().map(|set| bind(device, binding, set)).collect();

        Sets { sets, bind_groups }
    }

    /// Re-views every map at the new texture cap, behind a sampler rebuilt at the
    /// new anisotropy.
    pub(super) fn rebind(&mut self, device: &wgpu::Device, binding: Binding<'_>) {
        self.bind_groups = self
            .sets
            .iter()
            .map(|set| bind(device, binding, set))
            .collect();
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    appearance: &Appearance,
    resolved: &Resolved,
) -> Set {
    let maps = MapKind::ALL
        .iter()
        .map(|&kind| {
            let image = appearance.maps[kind.index()]
                .as_ref()
                .and_then(|reference| resolved.images.get(reference));
            let blank = solid(neutral(kind));
            texture::Uploaded::new(device, queue, image.unwrap_or(&blank), format(kind))
        })
        .collect();

    Set {
        maps,
        uniform: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview surface appearance"),
            contents: bytemuck::bytes_of(&Uniform {
                tint: [
                    appearance.tint[0],
                    appearance.tint[1],
                    appearance.tint[2],
                    f32::from(u8::from(appearance.alpha_mode == AlphaMode::Transparency)),
                ],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        }),
    }
}

fn bind(device: &wgpu::Device, binding: Binding<'_>, set: &Set) -> wgpu::BindGroup {
    let views: Vec<wgpu::TextureView> = set
        .maps
        .iter()
        .map(|map| map.view(binding.max_size))
        .collect();
    let entries: Vec<wgpu::BindGroupEntry<'_>> = views
        .iter()
        .enumerate()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .chain([
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(binding.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: set.uniform.as_entire_binding(),
            },
        ])
        .collect();

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview surface appearance"),
        layout: binding.layout,
        entries: &entries,
    })
}

fn solid(color: [u8; 4]) -> Image {
    Image {
        width: 1,
        height: 1,
        pixels: color.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_colour_map_leaves_the_part_colour_showing_through() {
        // Overlay mixes toward the map by its alpha, so a zero there is the
        // only neutral that changes nothing.
        assert_eq!(neutral(MapKind::Color), [255, 255, 255, 0]);
        assert_eq!(neutral(MapKind::Normal), [128, 128, 255, 255]);
        assert_eq!(neutral(MapKind::Metalness)[0], 0);
        assert!(neutral(MapKind::Roughness)[0] > 200);
    }

    #[test]
    fn only_the_colour_map_is_read_through_an_srgb_view() {
        assert_eq!(format(MapKind::Color), wgpu::TextureFormat::Rgba8UnormSrgb);
        for kind in [MapKind::Normal, MapKind::Metalness, MapKind::Roughness] {
            assert_eq!(format(kind), wgpu::TextureFormat::Rgba8Unorm);
        }
    }

    #[test]
    fn the_uniform_is_one_vec4_the_shader_can_bind() {
        assert_eq!(std::mem::size_of::<Uniform>(), 16);
    }
}
