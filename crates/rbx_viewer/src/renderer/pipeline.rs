//! Bind group 0 — the view-projection matrix, the lighting uniform and the
//! environment probe every pass shares — and the one function every surface
//! pipeline in this renderer is built by.

use glam::Mat4;

use super::envmap::EnvMap;
use super::instance::InstanceRaw;
use super::lighting::LightingRaw;
use super::mesh::Vertex;

pub(super) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// WGSL has no #include, so the shared shading model is concatenated in front of
// each surface shader instead. `concat!` does it at compile time, which keeps the
// shaders plain files a text editor can still highlight.
//
// The local lights come first in every one of them: WGSL has no forward
// declarations, and `shade` in lighting.wgsl calls into them. (`concat!` takes
// literals only, so the pair cannot be factored out into a constant.)
pub(super) const BOX_SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("material.wgsl"),
    include_str!("shader.wgsl")
);
pub(super) const FILEMESH_SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("material.wgsl"),
    include_str!("filemesh.wgsl")
);
pub(super) const APPEARANCE_SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("material.wgsl"),
    include_str!("appearance.wgsl")
);
pub(super) const DECAL_SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("textured.wgsl")
);
// The sky's own haze and halo (see sky.wgsl) reach no other pass: they are
// concatenated here alone, after the model they read the uniform from.
pub(super) const SKYBOX_SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("sky.wgsl"),
    include_str!("skybox.wgsl")
);

// The frame uniform (bind group 0, binding 0): the view-projection matrix,
// then the viewport's pixel size in a `vec4` (`xy`, `z` the `ForceField`
// shimmer's phase, `w` padding) so the outline shaders can expand their edges
// to a constant pixel width. Every
// other surface shader declares only the matrix and reads just the first 64
// bytes, which stay first.
const FRAME_SIZE: wgpu::BufferAddress = 64 + 16;

/// Blending for everything translucent: straight (non-premultiplied) alpha over
/// whatever is already in the target.
pub(super) const ALPHA_BLENDING: wgpu::BlendState = wgpu::BlendState::ALPHA_BLENDING;

/// What a scene pass draws into: the HDR format, and how many samples the quality
/// level asks for. A pipeline is built for both at once and neither can change
/// without it, so they travel as one argument rather than two.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Target {
    pub(super) format: wgpu::TextureFormat,
    pub(super) samples: u32,
}

impl Target {
    /// The multisample state this target stands for. One place, so a sample count
    /// is never set on one pipeline and forgotten on another.
    pub(super) fn multisample(self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.samples.max(1),
            ..Default::default()
        }
    }
}

/// The bind groups a surface pass shares whatever it draws: the frame at group
/// 0 and the material arrays at group 1. A pass's own image, if it has one,
/// sits above both at group 2 — see [`shape_pipelines`] for why the order matters.
#[derive(Clone, Copy)]
pub(super) struct Bindings<'a> {
    pub(super) frame: &'a wgpu::BindGroup,
    pub(super) materials: &'a wgpu::BindGroup,
}

/// Everything in bind group 0 the renderer owns once and every pass shares.
/// Only the matrix is per-pass, which is the whole reason [`Frame`] exists.
#[derive(Clone, Copy)]
pub(super) struct Shared<'a> {
    pub(super) lighting: &'a wgpu::Buffer,
    /// Every `PointLight`/`SpotLight`/`SurfaceLight` in the place, which the
    /// shading loops over per fragment (see `renderer::lighting::local`).
    pub(super) lights: &'a wgpu::Buffer,
    pub(super) env: &'a EnvMap,
    pub(super) shadow_map: &'a wgpu::TextureView,
    pub(super) shadow_sampler: &'a wgpu::Sampler,
    /// The local lights' own shadow array and this frame's per-light records —
    /// see `renderer::shadow::local`. Sampled through `shadow_sampler` above.
    pub(super) local_shadow_map: &'a wgpu::TextureView,
    pub(super) light_shadows: &'a wgpu::Buffer,
    /// The `PointLight` cubes and their per-face matrices — see
    /// `renderer::shadow::point`. Sampled through `shadow_sampler` too.
    pub(super) point_shadow_map: &'a wgpu::TextureView,
    pub(super) point_faces: &'a wgpu::Buffer,
    /// The opaque scene a refracting surface reads — see `renderer::post`.
    pub(super) refraction: &'a wgpu::TextureView,
}

/// One pass's copy of bind group 0.
///
/// Passes do not all use the same matrix — the skybox and the celestial bodies
/// need the camera's rotation without its translation — so each keeps its own
/// buffer, while the lighting uniform and the environment probe behind it are
/// shared resources handed in from the renderer.
pub(super) struct Frame {
    matrix: wgpu::Buffer,
    pub(super) bind_group: wgpu::BindGroup,
}

pub(super) fn frame_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview frame"),
        entries: &[
            uniform(0),
            uniform(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::Cube,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // The sun's shadow map and the comparison sampler that reads it.
            // Every surface pass shares them, so they live beside the lighting
            // uniform rather than in a group of their own.
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            // Storage rather than uniform: the array is as long as the place has
            // lights, which no uniform's fixed size could cover.
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The local lights' own shadow maps, one array layer per shadowed
            // `SpotLight`/`SurfaceLight` this frame — see
            // `renderer::shadow::local`. Read through the very same comparison
            // sampler (binding 5) the sun's map is; a `sampler` is not tied to
            // any one texture, so no second one is needed.
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            // One record per entry of the local light buffer above: which array
            // layer (if any) it was assigned this frame, and the view-projection
            // to read that layer with. Rewritten every frame — the nearest
            // lights to the camera are the ones that get a layer, and the camera
            // moves (see `renderer::shadow::local::select`).
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The `PointLight` cubes and the six matrices each is read back
            // through — see `renderer::shadow::point`. A second array
            // rather than more layers of binding 7's: the faces are half
            // the side, which is what keeps six of them affordable.
            wgpu::BindGroupLayoutEntry {
                binding: 9,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 10,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The scene as it stood after the opaque pass, which a `Glass`
            // surface bends what is behind it out of — see
            // `post::Targets::capture_refraction`. Read through the
            // environment probe's own filtering sampler at binding 3; one
            // unread texel stands here in a place with no glass.
            wgpu::BindGroupLayoutEntry {
                binding: 11,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// The lighting uniform, written afresh every frame: the camera moves, so its
/// contents are not constant even when the place's `Lighting` is.
pub(super) fn lighting_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rbxview lighting"),
        size: LightingRaw::SIZE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// How long the `ForceField` window takes to wander its whole path and come
/// back. Roblox only calls the motion's period "pretty big" (the material's
/// 2019 DevForum announcement), so the figure is this renderer's own.
const SHIMMER_SECONDS: f64 = 12.0;

/// Where in its cycle the shimmer is after `elapsed`, in `[0, 1)`.
///
/// Wrapped here in `f64` rather than handed to the shader as raw seconds: an
/// `f32` clock loses the precision a smooth wave needs within hours, and a
/// phase that wraps exactly never jumps.
pub(super) fn shimmer_phase(elapsed: std::time::Duration) -> f32 {
    (elapsed.as_secs_f64() / SHIMMER_SECONDS).fract() as f32
}

impl Frame {
    pub(super) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        shared: Shared<'_>,
    ) -> Self {
        let matrix = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview view projection"),
            size: FRAME_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = bind(device, layout, &matrix, shared);

        Frame { matrix, bind_group }
    }

    /// Rebuilds the group around a new set of shared resources — a re-viewed
    /// environment probe, a reallocated shadow map, a recapped light buffer —
    /// keeping this pass's own matrix buffer, which no quality level touches.
    pub(super) fn rebind(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        shared: Shared<'_>,
    ) {
        self.bind_group = bind(device, layout, &self.matrix, shared);
    }

    /// `shimmer` is [`shimmer_phase`]'s: 0 for a pass with no clock.
    pub(super) fn write(
        &self,
        queue: &wgpu::Queue,
        matrix: &Mat4,
        viewport: glam::Vec2,
        shimmer: f32,
    ) {
        let mut data = [0.0f32; 20];
        data[..16].copy_from_slice(&matrix.to_cols_array());
        data[16] = viewport.x;
        data[17] = viewport.y;
        data[18] = shimmer;
        queue.write_buffer(&self.matrix, 0, bytemuck::cast_slice(&data));
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    matrix: &wgpu::Buffer,
    shared: Shared<'_>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview frame"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: matrix.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shared.lighting.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&shared.env.view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&shared.env.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(shared.shadow_map),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(shared.shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: shared.lights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(shared.local_shadow_map),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: shared.light_shadows.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(shared.point_shadow_map),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: shared.point_faces.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::TextureView(shared.refraction),
            },
        ],
    })
}

/// Everything the surface pipelines differ by. They all share bind group 0, the
/// reversed-Z depth state and one colour target, so this is the whole of it.
pub(super) struct Surface<'a> {
    pub(super) label: &'a str,
    pub(super) shader: &'a str,
    pub(super) layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    pub(super) buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    /// `None` keeps both faces, which downloaded meshes need (they do not all
    /// agree on winding) and closed procedural shapes do not.
    pub(super) cull: Option<wgpu::Face>,
    /// Blend instead of replacing, and stop writing depth: what a part with a
    /// `Transparency` between 0 and 1 needs so it hides nothing behind it.
    pub(super) translucent: bool,
    pub(super) bias: wgpu::DepthBiasState,
    /// Reversed-Z (see camera.rs) makes closer mean a *bigger* depth value, so
    /// the usual `Less` becomes `Greater`. The decal pass needs `GreaterEqual`
    /// instead: it redraws a surface already in the depth buffer, so it has to
    /// win the tie rather than disappear.
    pub(super) compare: wgpu::CompareFunction,
    /// `TriangleList` for every surface pass, the outline passes included:
    /// their edges are expanded into screen-space quads (see
    /// `renderer::outline`) rather than drawn as one-pixel lines.
    pub(super) topology: wgpu::PrimitiveTopology,
}

impl Surface<'_> {
    /// Culled, opaque, unbiased: what a part pass starts from.
    pub(super) fn new<'a>(
        label: &'a str,
        shader: &'a str,
        layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
        buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    ) -> Surface<'a> {
        Surface {
            label,
            shader,
            layouts,
            buffers,
            cull: Some(wgpu::Face::Back),
            translucent: false,
            bias: wgpu::DepthBiasState::default(),
            compare: wgpu::CompareFunction::Greater,
            topology: wgpu::PrimitiveTopology::TriangleList,
        }
    }
}

pub(super) fn surface(
    device: &wgpu::Device,
    target: Target,
    surface: &Surface<'_>,
) -> wgpu::RenderPipeline {
    let shader = crate::gpu::shader(device, surface.label, surface.shader);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(surface.label),
        bind_group_layouts: surface.layouts,
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(surface.label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: surface.buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: surface.topology,
            cull_mode: surface.cull,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!surface.translucent),
            depth_compare: Some(surface.compare),
            stencil: wgpu::StencilState::default(),
            bias: surface.bias,
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target.format,
                blend: Some(if surface.translucent {
                    ALPHA_BLENDING
                } else {
                    wgpu::BlendState::REPLACE
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The two pipelines the instanced shapes draw through: opaque first, then the
/// blended one everything translucent goes to.
pub(super) fn shape_pipelines(
    device: &wgpu::Device,
    target: Target,
    frame: &wgpu::BindGroupLayout,
    materials: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    // The shared groups come first and a pass's own image (which these two have
    // none of) last, so every surface pipeline's layout is a prefix of every
    // other's. Switching pipelines within a pass then never leaves a set bound
    // under an incompatible layout *below* one the new pipeline uses — which
    // Vulkan lets disturb the lower sets, and which showed up as a file mesh
    // drawn right after a textured one sampling garbage lighting and materials.
    let layouts = [Some(frame), Some(materials)];
    let buffers = [Some(Vertex::layout()), Some(InstanceRaw::layout())];
    let opaque = Surface::new("rbxview shapes", BOX_SHADER, &layouts, &buffers);

    (
        surface(device, target, &opaque),
        surface(
            device,
            target,
            &Surface {
                label: "rbxview shapes (blended)",
                translucent: true,
                ..opaque
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_shimmer_phase_starts_at_zero_and_wraps_once_a_cycle() {
        assert_eq!(shimmer_phase(Duration::ZERO), 0.0);
        assert!((shimmer_phase(Duration::from_secs(3)) - 0.25).abs() < 1e-6);
        assert!(shimmer_phase(Duration::from_secs(12)).abs() < 1e-6);
        // A day in, the phase is still exact to well under a frame's step.
        let day = Duration::from_secs(86_400) + Duration::from_millis(1500);
        assert!((shimmer_phase(day) - 0.125).abs() < 1e-4);
    }
}
