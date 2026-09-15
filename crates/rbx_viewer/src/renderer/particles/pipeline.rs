//! The GPU-facing half of the particle pass: vertex layouts, the shader module
//! and the one render pipeline every emitter's billboards draw through.

use bytemuck::{Pod, Zeroable};

use super::super::pipeline::{Target, DEPTH_FORMAT};

/// A billboard's unrotated corner, in `[-1, 1]`: the vertex buffer every
/// instance is drawn against, laid out as a triangle strip so no index buffer
/// is needed.
pub(super) const QUAD_CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]];

const CORNER_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    1 => Float32x3,
    2 => Float32,
    3 => Float32x3,
    4 => Float32,
    5 => Float32,
    6 => Float32,
];

const SHADER: &str = include_str!("../particles.wgsl");

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct CameraRaw {
    pub(super) view_projection: [[f32; 4]; 4],
    pub(super) eye: [f32; 3],
    pub(super) _pad: f32,
}

/// One particle's GPU footprint. `ZOffset` is folded into `position` on the CPU
/// (see [`super::Particles::collect`]) rather than carried as its own field:
/// nudging the world position toward the eye needs the eye anyway, which the
/// vertex shader would otherwise have to redo per corner instead of once per
/// particle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct ParticleRaw {
    pub(super) position: [f32; 3],
    pub(super) size: f32,
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
    pub(super) rotation: f32,
    pub(super) light_emission: f32,
}

/// Blended with a premultiplied `(One, OneMinusSrcAlpha)` state on both
/// channels — see `particles.wgsl`'s `fs_main` for why this one blend state
/// covers both straight alpha and additive `LightEmission`.
const PARTICLE_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

pub(super) fn camera_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview particle camera"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

pub(super) fn create_pipeline(
    device: &wgpu::Device,
    target: Target,
    camera_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rbxview particles"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview particles"),
        bind_group_layouts: &[Some(camera_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &CORNER_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }),
    ];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview particles"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Read the depth opaque geometry already wrote, but never write to
            // it: two overlapping particles must both blend, which writing
            // depth would prevent for whichever drew second.
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target.format,
                blend: Some(PARTICLE_BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing but their order links the WGSL struct to this layout, so a field
    // added to one alone reads the neighbouring attribute instead of failing to
    // compile.
    #[test]
    fn the_shader_reads_the_light_emission_field_the_layout_supplies() {
        assert!(SHADER.contains("@location(6) light_emission: f32"));
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 6);
        assert_eq!(INSTANCE_ATTRIBUTES[5].shader_location, 6);
        assert_eq!(
            INSTANCE_ATTRIBUTES[5].offset + INSTANCE_ATTRIBUTES[5].format.size(),
            std::mem::size_of::<ParticleRaw>() as wgpu::BufferAddress
        );
    }

    #[test]
    fn the_quad_is_four_corners_for_a_triangle_strip() {
        assert_eq!(QUAD_CORNERS.len(), 4);
    }
}
