//! The GPU-facing half of the GUI overlay pass: the vertex layout, the
//! viewport uniform, the shader module and the one pipeline every rectangle
//! draws through.

use bytemuck::{Pod, Zeroable};

const SHADER: &str = include_str!("../gui.wgsl");

/// The viewport in pixels, which is all the "camera" a screen-space pass has.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct ViewportRaw {
    pub(super) size: [f32; 2],
    pub(super) padding: [f32; 2],
}

/// One corner of a rectangle, already in viewport pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct VertexRaw {
    pub(super) position: [f32; 2],
    pub(super) uv: [f32; 2],
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x3,
    3 => Float32,
];

/// Straight alpha, unlike every other blended pass in this renderer: a GUI
/// rectangle is composited over a finished, opaque frame rather than added
/// into an HDR buffer, so there is no `LightEmission` to route through the
/// alpha channel and no premultiplication to undo.
const GUI_BLEND: wgpu::BlendState = wgpu::BlendState::ALPHA_BLENDING;

pub(super) fn viewport_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview gui viewport"),
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

/// Built for the *display* format and a single sample, not for the scene's HDR
/// target: the overlay is drawn after the resolve, so a quality level that
/// changes the scene's sample count never has to rebuild it.
pub(super) fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    viewport_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rbxview gui"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview gui"),
        bind_group_layouts: &[Some(viewport_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview gui"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // A rectangle is emitted corner by corner without regard for
            // winding, and nothing here is ever seen edge-on anyway.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(GUI_BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
