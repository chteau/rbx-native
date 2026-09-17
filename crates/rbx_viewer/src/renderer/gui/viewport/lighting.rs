//! What the frame's parts are lit with, and the stand-ins bind group 0
//! needs for everything the frame's docs deny it.

use glam::Vec3;

use crate::lighting::{Fog, Lighting};
use crate::quality::QualityProfile;
use crate::renderer::envmap::{EnvMap, Probe};
use crate::renderer::lighting::local_lights_buffer;
use crate::renderer::pipeline::DEPTH_FORMAT;
use crate::renderer::shadow;
use crate::scene::GuiViewport;

/// Where classic fog would start and end: far enough that nothing in a frame
/// is ever faded, since its docs give it no fog at all.
pub(super) const NO_FOG: f32 = 1.0e9;
/// A probe of no sky: zero irradiance on every axis, one mip.
pub(super) const NO_SKY: Probe = Probe {
    top_mip: 0.0,
    irradiance: [[0.0; 4]; 6],
};

/// The parts of bind group 0 this pass declares but never reads.
pub(super) struct StandIns {
    pub(super) env: EnvMap,
    pub(super) shadow: wgpu::Texture,
    pub(super) local_shadow: wgpu::Texture,
    pub(super) shadow_sampler: wgpu::Sampler,
    pub(super) lights: wgpu::Buffer,
    pub(super) light_shadows: wgpu::Buffer,
}

impl StandIns {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quality: &QualityProfile,
    ) -> Self {
        let depth = |label, layers| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        StandIns {
            // No sky: the one grey texel a scene without a `Sky` gets, never
            // sampled here anyway with both environment scales at 0.
            env: EnvMap::new(device, queue, None, quality),
            shadow: depth("rbxview viewport frame shadow stand-in", 1),
            local_shadow: depth("rbxview viewport frame local shadow stand-in", 1),
            shadow_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rbxview viewport frame shadow stand-in"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                ..Default::default()
            }),
            lights: local_lights_buffer(device, &[]),
            light_shadows: shadow::local::buffer(device, 0),
        }
    }
}

/// The frame's three lighting properties as the shading model reads them,
/// with everything else it could do switched off: one lamp at `light` and
/// no fill, no shadow, no sky, no fog, unit exposure.
pub(super) fn lighting_of(viewport: &GuiViewport) -> Lighting {
    Lighting {
        sun_direction: viewport.light,
        sun_color: Vec3::from(viewport.light_color),
        fill_color: Vec3::ZERO,
        ambient: Vec3::from(viewport.ambient),
        exposure: 1.0,
        environment_diffuse: 0.0,
        environment_specular: 0.0,
        global_shadows: false,
        shadow_softness: 0.0,
        fog: Fog::Linear {
            color: Vec3::ZERO,
            start: NO_FOG,
            end: NO_FOG,
        },
        sky_tint: Vec3::ONE,
        star_fade: 0.0,
        ..Lighting::default()
    }
}
