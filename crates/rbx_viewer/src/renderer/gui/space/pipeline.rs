//! The GPU-facing half of the in-world GUI pass: the vertex layout, the camera
//! uniform and the two pipelines a canvas quad draws through — one depth
//! tested, one for `AlwaysOnTop`.

use bytemuck::{Pod, Zeroable};

use crate::renderer::pipeline::{Target, DEPTH_FORMAT};

const SHADER: &str = include_str!("../../gui_space.wgsl");

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct CameraRaw {
    pub(super) view_projection: [[f32; 4]; 4],
}

/// One corner of a canvas quad, already at its final world position.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct VertexRaw {
    pub(super) position: [f32; 3],
    pub(super) uv: [f32; 2],
    /// What the sampled canvas colour is multiplied by — `Brightness` under
    /// `LightInfluence`, see `scene::gui::space::brightness`. Per vertex
    /// rather than in a uniform because every canvas of a pass shares one
    /// vertex buffer and one bind group slot.
    pub(super) brightness: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x2,
    2 => Float32,
];

/// The same depth bias decals carry, and for the same reason: a `SurfaceGui`
/// lies exactly on the part face it is pinned to, so without a nudge toward
/// the camera the two z-fight. Positive because reversed-Z (see `camera.rs`)
/// makes closer mean a *bigger* depth value.
const DEPTH_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 16,
    slope_scale: 2.0,
    clamp: 0.0,
};

pub(super) fn camera_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview gui space camera"),
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

/// `(depth tested, always on top)`. The pair differs only in its depth
/// compare: `AlwaysOnTop` is exactly "never occluded by 3D content".
pub(super) fn create_pipelines(
    device: &wgpu::Device,
    target: Target,
    camera_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    (
        create_pipeline(
            device,
            target,
            camera_layout,
            image_layout,
            wgpu::CompareFunction::Greater,
        ),
        create_pipeline(
            device,
            target,
            camera_layout,
            image_layout,
            wgpu::CompareFunction::Always,
        ),
    )
}

fn create_pipeline(
    device: &wgpu::Device,
    target: Target,
    camera_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rbxview gui space"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview gui space"),
        bind_group_layouts: &[Some(camera_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview gui space"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // A `SurfaceGui` on a face the camera happens to be behind still
            // shows in Roblox, and a corner order is not a winding promise.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Read-only, like every other blended pass: two canvases that
            // overlap must both blend rather than whichever drew first winning.
            depth_write_enabled: Some(false),
            depth_compare: Some(depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: DEPTH_BIAS,
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
