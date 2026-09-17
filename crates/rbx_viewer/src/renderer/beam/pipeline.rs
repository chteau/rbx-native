//! The GPU-facing half of the beam pass: the vertex layout, the shader module
//! and the one render pipeline every beam's ribbon draws through.
//!
//! No instancing and no shared per-vertex "corner" buffer, unlike
//! `renderer::particles::pipeline`: a ribbon's shape already varies per beam
//! (segment count, curve, width taper), so [`super::ribbon`] builds its final
//! world-space vertices on the CPU and this is a plain vertex buffer.

use bytemuck::{Pod, Zeroable};

use super::super::pipeline::{Target, DEPTH_FORMAT};

const SHADER: &str = include_str!("../beam.wgsl");

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct CameraRaw {
    pub(super) view_projection: [[f32; 4]; 4],
    /// The light a `LightInfluence`-1 beam is tinted by (rgb; `w` padding) —
    /// see `beam.wgsl`.
    pub(super) env_light: [f32; 4],
}

/// One ribbon vertex, already at its final world position — see
/// `ribbon::vertices`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct VertexRaw {
    pub(super) position: [f32; 3],
    pub(super) uv: [f32; 2],
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
    pub(super) light_emission: f32,
    pub(super) light_influence: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Float32x3,
    3 => Float32,
    4 => Float32,
    5 => Float32,
];

/// Same premultiplied `(One, OneMinusSrcAlpha)` state particles use — see
/// `renderer::particles::pipeline::PARTICLE_BLEND`.
const BEAM_BLEND: wgpu::BlendState = wgpu::BlendState {
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
        label: Some("rbxview beam camera"),
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
        label: Some("rbxview beams"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview beams"),
        bind_group_layouts: &[Some(camera_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview beams"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            // Both faces: a fixed-orientation (non-`FaceCamera`) ribbon is
            // routinely seen edge-on from behind, and the degenerate bridge
            // triangles `ribbon::build` inserts between beams in one texture
            // group can flip strip winding — neither should ever cull.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Same read-only depth test as particles: two overlapping beams
            // must both blend rather than whichever drew first winning.
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
                blend: Some(BEAM_BLEND),
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

    #[test]
    fn the_shader_reads_the_light_emission_field_the_layout_supplies() {
        assert!(SHADER.contains("@location(4) light_emission: f32"));
        assert!(SHADER.contains("@location(5) light_influence: f32"));
        assert_eq!(VERTEX_ATTRIBUTES.len(), 6);
        assert_eq!(VERTEX_ATTRIBUTES[5].shader_location, 5);
        assert_eq!(
            VERTEX_ATTRIBUTES[5].offset + VERTEX_ATTRIBUTES[5].format.size(),
            std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress
        );
    }
}
