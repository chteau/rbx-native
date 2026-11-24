//! The `Highlight` pass's GPU scaffolding: the mask target, the four
//! pipelines that draw into it, and the two that read it back out.
//!
//! Six mask pipelines rather than one because two things vary independently
//! and neither can be a uniform: the vertex stride (a unit shape carries a
//! normal beside its position, a file mesh carries the position alone, the
//! same split `renderer::shadow` has) and the depth test, the two
//! `Enum.HighlightDepthMode` picks between plus the visible half of the
//! editor cue's `DepthMode::Faded`.
//!
//! Two composite pipelines for the reason `renderer::post` has two of its
//! own: the mask shares the scene's sample count, because it is drawn in a
//! pass that attaches the scene's depth buffer, and WGSL types a multisampled
//! texture as a different type from a single-sampled one.

use bytemuck::{Pod, Zeroable};

use super::super::mesh::Vertex;
use super::super::pipeline::{Target, DEPTH_FORMAT};
use super::super::scene_depth;
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

/// One highlight's two colours as the composite reads them, `Paint` in
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

/// The mask pipelines, by geometry kind and depth test.
pub(super) struct Mask {
    shapes_on_top: wgpu::RenderPipeline,
    shapes_occluded: wgpu::RenderPipeline,
    shapes_visible: wgpu::RenderPipeline,
    meshes_on_top: wgpu::RenderPipeline,
    meshes_occluded: wgpu::RenderPipeline,
    meshes_visible: wgpu::RenderPipeline,
    /// The editor's cues mark what is visible for an `Occluded` claim too,
    /// so a part hidden behind an opaque adornment loses its outline the
    /// same as one hidden behind a part.
    onto: Onto,
}

/// What one mask pipeline tests and writes.
#[derive(Clone, Copy)]
enum Test {
    OnTop,
    Occluded,
    /// The index with `VISIBLE` set where nothing opaque stands in front,
    /// parts or depth-tested adornments (`highlight.wgsl`'s `fs_visible`).
    Visible,
}

impl Mask {
    pub(super) fn new(
        device: &wgpu::Device,
        frame: &wgpu::BindGroupLayout,
        target: Target,
        onto: Onto,
    ) -> Self {
        let shader = crate::gpu::shader(
            device,
            "rbxview highlight mask",
            include_str!("../highlight.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview highlight mask"),
            bind_group_layouts: &[Some(frame)],
            immediate_size: 0,
        });
        let visible_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview highlight mask (visible)"),
            bind_group_layouts: &[Some(frame), Some(&scene_depth::layout(device, 1))],
            immediate_size: 0,
        });
        let shape_stride = std::mem::size_of::<Vertex>() as wgpu::BufferAddress;
        let mesh_stride = std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress;
        let build = |stride, test| {
            let layout = match test {
                Test::Visible => &visible_layout,
                _ => &layout,
            };
            mask_pipeline(device, &shader, layout, target, stride, test)
        };

        Mask {
            shapes_on_top: build(shape_stride, Test::OnTop),
            shapes_occluded: build(shape_stride, Test::Occluded),
            shapes_visible: build(shape_stride, Test::Visible),
            meshes_on_top: build(mesh_stride, Test::OnTop),
            meshes_occluded: build(mesh_stride, Test::Occluded),
            meshes_visible: build(mesh_stride, Test::Visible),
            onto,
        }
    }

    /// The pipeline one claim's mask is drawn with, and for a `Faded`
    /// claim the second one that marks which of what the first covered is
    /// visible.
    pub(super) fn shapes(&self, mode: DepthMode) -> Passes<'_> {
        match mode {
            DepthMode::AlwaysOnTop => (&self.shapes_on_top, None),
            DepthMode::Occluded if self.onto == Onto::Frame => (&self.shapes_visible, None),
            DepthMode::Occluded => (&self.shapes_occluded, None),
            DepthMode::Faded => (&self.shapes_on_top, Some(&self.shapes_visible)),
        }
    }

    pub(super) fn meshes(&self, mode: DepthMode) -> Passes<'_> {
        match mode {
            DepthMode::AlwaysOnTop => (&self.meshes_on_top, None),
            DepthMode::Occluded if self.onto == Onto::Frame => (&self.meshes_visible, None),
            DepthMode::Occluded => (&self.meshes_occluded, None),
            DepthMode::Faded => (&self.meshes_on_top, Some(&self.meshes_visible)),
        }
    }
}

/// See [`Mask::shapes`].
pub(super) type Passes<'a> = (&'a wgpu::RenderPipeline, Option<&'a wgpu::RenderPipeline>);

/// The composite's pipeline, its one bind group built against
/// [`composite_layout`] at the mask's sample count.
pub(super) struct Composite {
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// Where a composite paints.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::renderer) enum Onto {
    /// The HDR scene, before the resolve, the way a `Highlight` is drawn.
    Scene,
    /// The finished frame through its non-sRGB view, in sRGB values: the
    /// editor's cues, which read `DepthMode::Faded`'s visible marks.
    Frame,
}

impl Composite {
    /// Paints into `color` from a mask of `mask_samples`.
    pub(super) fn new(device: &wgpu::Device, color: Target, mask_samples: u32, onto: Onto) -> Self {
        let multisampled = mask_samples > 1;
        let source = include_str!("../highlight_composite.wgsl");
        let shader = crate::gpu::shader(
            device,
            "rbxview highlight composite",
            &match multisampled {
                true => source.replace(MASK_BINDING, MULTISAMPLED_MASK_BINDING),
                false => source.to_string(),
            },
        );
        let layout = composite_layout(device, mask_samples);
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
                multisample: color.multisample(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: match onto {
                            Onto::Scene => &[],
                            Onto::Frame => &[("ON_FRAME", 1.0)],
                        },
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            }),
        }
    }
}

/// The mask texture itself, at the frame's size and the scene's sample count
/// it shares a render pass with the scene's own depth buffer, which fixes
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
    test: Test,
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
            depth_compare: Some(match test {
                // Reversed-Z (see `camera.rs`) puts nearer surfaces at the
                // larger depth, so "nothing in front of it" is `GreaterEqual`
                // equal because this re-draws the very geometry that wrote
                // the value it is testing against.
                Test::Occluded | Test::Visible => wgpu::CompareFunction::GreaterEqual,
                Test::OnTop => wgpu::CompareFunction::Always,
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(match test {
                Test::Visible => "fs_visible",
                _ => "fs_main",
            }),
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

/// The composite's one bind group layout, the mask at the sample count it
/// was drawn at and the paints.
pub(super) fn composite_layout(device: &wgpu::Device, mask_samples: u32) -> wgpu::BindGroupLayout {
    let multisampled = mask_samples > 1;
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    })
}
