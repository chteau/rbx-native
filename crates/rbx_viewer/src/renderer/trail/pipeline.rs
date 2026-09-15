//! The GPU-facing half of the trail pass: the vertex layout, the shader
//! module and the one render pipeline every trail's ribbon draws through.
//!
//! Layout, blend state and shader are identical to `renderer::beam::pipeline`
//! by design (a trail ribbon and a beam ribbon are shaded the same way — see
//! `trail.wgsl`'s doc); kept as its own copy rather than shared because
//! `renderer::beam::pipeline`'s items are `pub(super)`, scoped to
//! `renderer::beam` only.

use bytemuck::{Pod, Zeroable};

use super::super::pipeline::{Target, DEPTH_FORMAT};

const SHADER: &str = include_str!("../trail.wgsl");

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct CameraRaw {
    pub(super) view_projection: [[f32; 4]; 4],
}

/// One ribbon vertex, already at its final world position — see
/// `super::ribbon::vertices`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct VertexRaw {
    pub(super) position: [f32; 3],
    pub(super) uv: [f32; 2],
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
    pub(super) light_emission: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Float32x3,
    3 => Float32,
    4 => Float32,
];

/// Same premultiplied `(One, OneMinusSrcAlpha)` state beams and particles
/// use — see `renderer::particles::pipeline::PARTICLE_BLEND`.
const TRAIL_BLEND: wgpu::BlendState = wgpu::BlendState {
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
        label: Some("rbxview trail camera"),
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
        label: Some("rbxview trails"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview trails"),
        bind_group_layouts: &[Some(camera_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview trails"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            // Both faces: a trail is always seen from either side once
            // `FaceCamera` turns it toward the eye, and the degenerate bridge
            // triangles between texture-grouped trails can flip strip winding
            // — see `renderer::beam::pipeline`'s identical reasoning.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Read-only depth, same as beams: two overlapping trails must
            // both blend rather than whichever drew first winning.
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
                blend: Some(TRAIL_BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
