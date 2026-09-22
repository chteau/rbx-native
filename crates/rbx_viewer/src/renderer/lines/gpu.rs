//! The line pass's pipelines, and the stencil the transform handles are
//! stamped into first so no guide is drawn over them.

use super::super::adornment::LineVertex;
use super::super::gizmo;
use super::super::pipeline;
use super::DotVertex;

const SHADER: &str = include_str!("../lines.wgsl");
const HANDLE_SHADER: &str = include_str!("../gizmo.wgsl");
/// What a multisampled scene depth buffer changes in [`SHADER`]: WGSL types
/// it differently, and it has more than one depth per pixel to reach the
/// farthest of — the substitution `renderer::post` makes for its resolve,
/// plus the loop.
const DEPTH_BINDING: &str = "var scene_depth: texture_depth_2d";
const MULTISAMPLED_DEPTH_BINDING: &str = "var scene_depth: texture_depth_multisampled_2d";
const DEPTH_READ: &str = "    return textureLoad(scene_depth, pixel, 0);";
const MULTISAMPLED_DEPTH_READ: &str = "    var farthest = 1.0;
    for (var sample = 0u; sample < textureNumSamples(scene_depth); sample++) {
        farthest = min(farthest, textureLoad(scene_depth, pixel, i32(sample)));
    }
    return farthest;";

pub(super) const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;
/// What a handle leaves in the stencil, and what a guide must not find there.
pub(super) const HANDLE: u32 = 1;

/// The pipelines for one target format and one scene sample count — lines
/// then dots, each depth-tested then drawn through — the handle stamp, and
/// the layout the scene's depth buffer is bound through.
pub(super) struct Gpu {
    pub(super) format: wgpu::TextureFormat,
    pub(super) samples: u32,
    pub(super) depth_layout: wgpu::BindGroupLayout,
    pub(super) pipelines: [[wgpu::RenderPipeline; 2]; 2],
    pub(super) handles: wgpu::RenderPipeline,
}

impl Gpu {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let multisampled = samples > 1;
        let source = if multisampled {
            SHADER
                .replace(DEPTH_BINDING, MULTISAMPLED_DEPTH_BINDING)
                .replace(DEPTH_READ, MULTISAMPLED_DEPTH_READ)
        } else {
            SHADER.to_owned()
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview lines"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbxview lines depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled,
                },
                count: None,
            }],
        });
        let frame = pipeline::frame_layout(device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rbxview lines"),
            bind_group_layouts: &[Some(&frame), Some(&depth_layout)],
            immediate_size: 0,
        });
        // A guide is kept off every pixel a handle stamped: Studio draws its
        // handles over its guides, and these are drawn after the handles.
        let guide_stencil = stencil(
            wgpu::CompareFunction::NotEqual,
            wgpu::StencilOperation::Keep,
        );
        let build = |vertex: &str, fragment: &str, buffer: wgpu::VertexBufferLayout<'_>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(buffer)],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(guide_stencil.clone()),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let handle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview lines handles"),
            source: wgpu::ShaderSource::Wgsl(HANDLE_SHADER.into()),
        });
        // The handles' own triangles, colourless: only the stencil is written.
        // The colour target is still declared, since the pass has one.
        let handles = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rbxview lines handles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &handle_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(gizmo::vertex_layout())],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil(
                wgpu::CompareFunction::Always,
                wgpu::StencilOperation::Replace,
            )),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &handle_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Gpu {
            format,
            samples,
            depth_layout,
            pipelines: [
                [
                    build("vs_line", "fs_line", LineVertex::layout()),
                    build("vs_line", "fs_line_on_top", LineVertex::layout()),
                ],
                [
                    build("vs_dot", "fs_dot", DotVertex::layout()),
                    build("vs_dot", "fs_dot_on_top", DotVertex::layout()),
                ],
            ],
            handles,
        }
    }
}

/// A stencil-only state: no depth at all, `compare` against the reference,
/// and `pass` on success.
fn stencil(
    compare: wgpu::CompareFunction,
    pass: wgpu::StencilOperation,
) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: pass,
    };
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The stencil the handles are stamped into: the display target's size, one
/// sample like it.
pub(super) fn stencil_target(device: &wgpu::Device, size: wgpu::Extent3d) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("rbxview lines stencil"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
