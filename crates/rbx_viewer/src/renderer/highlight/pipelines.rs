//! The `Highlight` pass's GPU scaffolding: the mask target, the four
//! pipelines that draw into it, and the two that read it back out.
//!
//! Four mask pipelines rather than one because two things vary independently
//! and neither can be a uniform: the vertex stride (a unit shape carries a
//! normal beside its position, a file mesh carries the position alone — the
//! same split `renderer::shadow` has) and the depth test that
//! `Enum.HighlightDepthMode` picks between.
//!
//! Two composite pipelines for the reason `renderer::post` has two of its
//! own: the mask shares the scene's sample count, because it is drawn in a
//! pass that attaches the scene's depth buffer, and WGSL types a multisampled
//! texture as a different type from a single-sampled one.

use bytemuck::{Pod, Zeroable};

use super::super::mesh::Vertex;
use super::super::pipeline::{Target, DEPTH_FORMAT};
use crate::scene::{DepthMode, Highlight, MAX_HIGHLIGHTS};

/// The mask is one 1-based highlight index per pixel, and there are never
/// more than 255 highlights (see `scene::highlight::MAX_HIGHLIGHTS`), so a
/// byte holds every value the pass can write with room for the zero that
/// means "none".
pub(super) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;

const MASK_BINDING: &str = "var mask: texture_2d<u32>";
const MULTISAMPLED_MASK_BINDING: &str = "var mask: texture_multisampled_2d<u32>";

const POSITION_ATTRIBUTE: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Uint32,
];

/// One instance of one highlighted piece of geometry: where it stands, and
/// which highlight claims it.
///
/// Deliberately not `super::super::instance::InstanceRaw`: a mask needs no
/// colour, material or reflectance, and does need the index that one has no
/// room for.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct MaskInstance {
    model: [[f32; 4]; 4],
    /// 1-based; see `highlight.wgsl` for why zero is not a highlight.
    index: u32,
}

impl MaskInstance {
    pub(super) fn new(model: [[f32; 4]; 4], index: u32) -> Self {
        MaskInstance { model, index }
    }
}

/// One highlight's two colours as the composite reads them — `Paint` in
/// `highlight_composite.wgsl`. The alphas are already `1 - Transparency`;
/// see `scene::highlight::Highlight`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Paint {
    fill: [f32; 4],
    outline: [f32; 4],
}

impl Paint {
    pub(super) fn of(highlight: &Highlight) -> Self {
        let [fill_r, fill_g, fill_b] = highlight.fill;
        let [line_r, line_g, line_b] = highlight.outline;
        Paint {
            fill: [fill_r, fill_g, fill_b, highlight.fill_alpha],
            outline: [line_r, line_g, line_b, highlight.outline_alpha],
        }
    }
}

/// Always the full 255 entries: a uniform array is a fixed length in WGSL, and
/// 8KB of mostly-unread colours is cheaper than a second pipeline per length.
pub(super) const PAINTS_SIZE: u64 = (MAX_HIGHLIGHTS * std::mem::size_of::<Paint>()) as u64;

/// The four mask pipelines, by geometry kind and depth mode.
pub(super) struct Mask {
    shapes_on_top: wgpu::RenderPipeline,
    shapes_occluded: wgpu::RenderPipeline,
    meshes_on_top: wgpu::RenderPipeline,
    meshes_occluded: wgpu::RenderPipeline,
}

impl Mask {
    pub(super) fn new(
        device: &wgpu::Device,
        frame: &wgpu::BindGroupLayout,
        target: Target,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview highlight mask"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../highlight.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview highlight mask"),
            bind_group_layouts: &[Some(frame)],
            immediate_size: 0,
        });
        let shape_stride = std::mem::size_of::<Vertex>() as wgpu::BufferAddress;
        let mesh_stride = std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress;

        Mask {
            shapes_on_top: mask_pipeline(
                device,
                &shader,
                &layout,
                target,
                shape_stride,
                DepthMode::AlwaysOnTop,
            ),
            shapes_occluded: mask_pipeline(
                device,
                &shader,
                &layout,
                target,
                shape_stride,
                DepthMode::Occluded,
            ),
            meshes_on_top: mask_pipeline(
                device,
                &shader,
                &layout,
                target,
                mesh_stride,
                DepthMode::AlwaysOnTop,
            ),
            meshes_occluded: mask_pipeline(
                device,
                &shader,
                &layout,
                target,
                mesh_stride,
                DepthMode::Occluded,
            ),
        }
    }

    pub(super) fn shapes(&self, mode: DepthMode) -> &wgpu::RenderPipeline {
        match mode {
            DepthMode::AlwaysOnTop => &self.shapes_on_top,
            DepthMode::Occluded => &self.shapes_occluded,
        }
    }

    pub(super) fn meshes(&self, mode: DepthMode) -> &wgpu::RenderPipeline {
        match mode {
            DepthMode::AlwaysOnTop => &self.meshes_on_top,
            DepthMode::Occluded => &self.meshes_occluded,
        }
    }
}

/// The composite's pipeline and the layout its one bind group is built
/// against, both already matched to the mask's sample count.
pub(super) struct Composite {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) pipeline: wgpu::RenderPipeline,
}

impl Composite {
    pub(super) fn new(device: &wgpu::Device, target: Target) -> Self {
        let multisampled = target.samples > 1;
        let source = include_str!("../highlight_composite.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview highlight composite"),
            source: wgpu::ShaderSource::Wgsl(match multisampled {
                true => source
                    .replace(MASK_BINDING, MULTISAMPLED_MASK_BINDING)
                    .into(),
                false => source.into(),
            }),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbxview highlight composite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview highlight composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        Composite {
            pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rbxview highlight composite"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                // No depth at all: the mask pass has already decided what each
                // pixel is allowed to show, so the composite only paints.
                depth_stencil: None,
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
            }),
            layout,
        }
    }
}

/// The mask texture itself, at the frame's size and the scene's sample count
/// — it shares a render pass with the scene's own depth buffer, which fixes
/// both.
pub(super) fn mask_texture(
    device: &wgpu::Device,
    size: (u32, u32),
    samples: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("rbxview highlight mask"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples.max(1),
            dimension: wgpu::TextureDimension::D2,
            format: MASK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn mask_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target: Target,
    stride: wgpu::BufferAddress,
    mode: DepthMode,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview highlight mask"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: stride,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &POSITION_ATTRIBUTE,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MaskInstance>() as _,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRIBUTES,
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            // A downloaded mesh's winding is not dependable (see
            // `renderer::filemesh`), and a silhouette that dropped half a
            // model's faces would have holes in it.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Never written: the scene's depth is finished by the time this
            // runs, and the passes after it still read what the scene left.
            depth_write_enabled: Some(false),
            depth_compare: Some(match mode {
                // Reversed-Z (see `camera.rs`) puts nearer surfaces at the
                // larger depth, so "nothing in front of it" is `GreaterEqual`
                // — equal because this re-draws the very geometry that wrote
                // the value it is testing against.
                DepthMode::Occluded => wgpu::CompareFunction::GreaterEqual,
                DepthMode::AlwaysOnTop => wgpu::CompareFunction::Always,
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: MASK_FORMAT,
                // An index is a name, not a quantity: blending two of them
                // would make up a third.
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
