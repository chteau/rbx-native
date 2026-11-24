//! The adornment passes' pipelines and vertex layouts: a solid one, a
//! screen-expanded line one and a textured one, for the layer the
//! depth-tested adornments are drawn into (opaque, writing the adornment
//! depth, and blended, not) and for the always-on-top pass.
//!
//! Built the first frame a place actually draws an adornment (see
//! [`super::Adornments`]), so a place with none compiles none of them.

use super::{ImageVertex, LineVertex, Vertex};
use crate::renderer::pipeline::{self, ALPHA_BLENDING, DEPTH_FORMAT};
use crate::renderer::post::ADORNMENT_LAYER_FORMAT;
use crate::renderer::scene_depth;

const SOLID_SHADER: &str = include_str!("../adornment.wgsl");
const LINE_SHADER: &str = include_str!("../adornment_line.wgsl");
const IMAGE_SHADER: &str = include_str!("../adornment_image.wgsl");

/// The transform tool's handles in the layer: a hole wherever one stands in
/// front of every depth-tested adornment so far, so the resolve shows the
/// handle the scene pass drew there, and the handle's depth for the
/// adornments drawn after it to test against (see `super::passes`).
const HANDLE_SHADER: &str = "
@group(0) @binding(0) var<uniform> view_proj: mat4x4<f32>;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return view_proj * vec4<f32>(position, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
";

/// Premultiplied "over": the layer accumulates every depth-tested
/// adornment over the ones behind it, and the resolve lays the sum over the
/// frame in one go, which "over" being associative makes the same thing.
const PREMULTIPLIED: wgpu::BlendState = wgpu::BlendState {
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

/// One pipeline per vertex kind.
pub(super) struct Set {
    pub(super) solid: wgpu::RenderPipeline,
    pub(super) line: wgpu::RenderPipeline,
    pub(super) image: wgpu::RenderPipeline,
}

/// The depth-tested adornments' pipelines, built for the scene's sample
/// count, which is all the shape of the scene depth they read depends on.
pub(super) struct Layer {
    opaque: Set,
    blended: Set,
    pub(super) handles: wgpu::RenderPipeline,
}

impl Layer {
    pub(super) fn new(device: &wgpu::Device, samples: u32, images: &wgpu::BindGroupLayout) -> Self {
        let build = |depth_write| {
            Set::new(
                device,
                Variant::Layer {
                    samples,
                    depth_write,
                },
                images,
            )
        };
        Layer {
            opaque: build(true),
            blended: build(false),
            handles: handles(device),
        }
    }

    pub(super) fn set(&self, opaque: bool) -> &Set {
        if opaque {
            &self.opaque
        } else {
            &self.blended
        }
    }
}

/// The always-on-top adornments' pipelines, for the frame's own format.
pub(super) struct Top(pub(super) Set);

impl Top {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        images: &wgpu::BindGroupLayout,
    ) -> Self {
        Top(Set::new(device, Variant::Top { format }, images))
    }
}

#[derive(Clone, Copy)]
enum Variant {
    Layer { samples: u32, depth_write: bool },
    Top { format: wgpu::TextureFormat },
}

impl Set {
    fn new(device: &wgpu::Device, variant: Variant, images: &wgpu::BindGroupLayout) -> Self {
        let frame = pipeline::frame_layout(device);
        Set {
            solid: build(
                device,
                variant,
                "rbxview adornment",
                SOLID_SHADER,
                Vertex::layout(),
                &frame,
                None,
            ),
            line: build(
                device,
                variant,
                "rbxview adornment line",
                LINE_SHADER,
                LineVertex::layout(),
                &frame,
                None,
            ),
            image: build(
                device,
                variant,
                "rbxview adornment image",
                IMAGE_SHADER,
                ImageVertex::layout(),
                &frame,
                Some(images),
            ),
        }
    }
}

/// One adornment pipeline, unculled (a single-sided shape drops its back
/// faces in the shader, see `super`) and depth-tested `GreaterEqual`
/// against the adornment depth: reversed-Z, so "in front of, or exactly on",
/// the tie going to the adornment drawn later. A layer one also tests
/// itself against the scene's depth in the shader (group 3).
fn build(
    device: &wgpu::Device,
    variant: Variant,
    label: &str,
    shader: &str,
    buffer: wgpu::VertexBufferLayout<'_>,
    frame: &wgpu::BindGroupLayout,
    images: Option<&wgpu::BindGroupLayout>,
) -> wgpu::RenderPipeline {
    let (format, blend, depth_write, samples, entry) = match variant {
        Variant::Layer {
            samples,
            depth_write,
        } => (
            ADORNMENT_LAYER_FORMAT,
            PREMULTIPLIED,
            depth_write,
            samples,
            "fs_layer",
        ),
        Variant::Top { format } => (format, ALPHA_BLENDING, false, 1, "fs_top"),
    };
    let reads = matches!(variant, Variant::Layer { .. });
    let source = scene_depth::with_source(shader, reads, samples);
    let module = crate::gpu::shader(device, label, &source);
    let depth = scene_depth::layout(device, samples);
    let mut layouts = vec![Some(frame), images];
    if reads {
        layouts.extend([None, Some(&depth)]);
    }
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(buffer)],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn handles(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let label = "rbxview adornment handles";
    let module = crate::gpu::shader(device, label, HANDLE_SHADER);
    let frame = pipeline::frame_layout(device);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&frame)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(crate::renderer::gizmo::vertex_layout())],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: ADORNMENT_LAYER_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

const SOLID_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32];
const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32, 4 => Float32x4];
const IMAGE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32];

impl Vertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SOLID_ATTRIBUTES,
        }
    }
}

impl LineVertex {
    /// Shared with `renderer::lines`, whose lines are built by the same
    /// `geometry::line`.
    pub(in crate::renderer) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &LINE_ATTRIBUTES,
        }
    }
}

impl ImageVertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &IMAGE_ATTRIBUTES,
        }
    }
}
