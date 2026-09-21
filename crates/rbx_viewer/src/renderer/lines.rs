//! Plain world-space line segments an editor draws over the scene — Studio's
//! light guides and dragger guides. Nothing here knows what a segment means:
//! the embedder works out where each one goes and hands the lot over, the
//! way `renderer::preview` takes bare boxes.
//!
//! Drawn onto the finished frame after the tone map, not into the HDR scene
//! with the adornments: Studio's guides are single pixels of flat colour, and
//! a line that thin drawn into the scene came out multisampled across two
//! pixels, blended, bloomed and tone mapped into a grey smear. A segment
//! whose two ends coincide is a dot instead — Studio's
//! `SphereHandleAdornment` markers, which its draggers size to stay constant
//! on screen. Costs nothing until the first segment arrives — the pipelines
//! are built then, not before — and draws nothing once the list is cleared
//! again.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::adornment::{line, Batch, LineVertex};
use super::pipeline;
use super::post::Targets;

const SHADER: &str = include_str!("lines.wgsl");
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

/// One line an editor overlay draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: Vec3,
    pub to: Vec3,
    /// Linear RGB, and alpha.
    pub color: [f32; 4],
    /// Drawn through whatever stands in front of it rather than hidden by it.
    pub on_top: bool,
    /// Across, on screen, in pixels. With `from == to`, the diameter of the
    /// round dot drawn there.
    pub width: f32,
}

/// One corner of a dot's screen-space square: its centre in the world, which
/// corner (`±1, ±1`), and its radius in pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DotVertex {
    centre: [f32; 3],
    corner: [f32; 2],
    radius: f32,
    color: [f32; 4],
}

const DOT_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Float32x4];

impl DotVertex {
    const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DotVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &DOT_ATTRIBUTES,
        }
    }
}

fn dot(centre: Vec3, width: f32, color: [f32; 4], out: &mut Vec<DotVertex>) {
    let corner = |x: f32, y: f32| DotVertex {
        centre: centre.to_array(),
        corner: [x, y],
        radius: width * 0.5,
        color,
    };
    out.extend([
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]);
}

#[derive(Default)]
pub(super) struct Lines {
    gpu: Option<Gpu>,
    /// Each layer's lines and dots, uploaded apart so that replacing one
    /// layer leaves the others' buffers alone.
    layers: Vec<Layer>,
}

#[derive(Default)]
struct Layer {
    lines: Batch,
    dots: Batch,
}

impl Layer {
    fn batches(&self) -> [&Batch; 2] {
        [&self.lines, &self.dots]
    }
}

impl Lines {
    /// Replaces one layer's segments. An empty list clears it.
    pub(super) fn set(&mut self, device: &wgpu::Device, layer: usize, segments: &[Segment]) {
        let mut sides = [Vec::new(), Vec::new()];
        let mut dots = [Vec::new(), Vec::new()];
        for segment in segments {
            let side = usize::from(segment.on_top);
            if segment.from == segment.to {
                dot(segment.from, segment.width, segment.color, &mut dots[side]);
            } else {
                line(
                    segment.from,
                    segment.to,
                    segment.width,
                    segment.color,
                    &mut sides[side],
                );
            }
        }
        if self.layers.len() <= layer {
            self.layers.resize_with(layer + 1, Layer::default);
        }
        let [occluded, on_top] = sides;
        self.layers[layer].lines = Batch::build(device, "rbxview lines", &occluded, &on_top);
        let [occluded, on_top] = dots;
        self.layers[layer].dots = Batch::build(device, "rbxview dots", &occluded, &on_top);
    }

    /// Draws every layer onto `target`, the frame the resolve just finished:
    /// the depth-tested lines first, then the ones drawn through, then the
    /// dots over the lines they mark — every layer's lines before any
    /// layer's dots.
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        scene: &Targets,
        frame: &wgpu::BindGroup,
    ) {
        let drawn = |batch: &Batch| batch.range(false).is_some() || batch.range(true).is_some();
        if !self
            .layers
            .iter()
            .any(|layer| layer.batches().into_iter().any(drawn))
        {
            return;
        }
        let format = target.format().remove_srgb_suffix();
        let samples = scene.samples();
        if self
            .gpu
            .as_ref()
            .is_none_or(|gpu| (gpu.format, gpu.samples) != (format, samples))
        {
            self.gpu = Some(Gpu::new(device, format, samples));
        }
        let Some(gpu) = &self.gpu else {
            return;
        };

        // Made per frame rather than kept: the depth buffer is replaced
        // whenever the frame is resized or its sample count changes, and a
        // bind group to the old one would draw against a stale depth.
        let depth = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview lines depth"),
            layout: &gpu.depth_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene.depth()),
            }],
        });
        // The non-sRGB view, so the blend works on encoded values — see
        // `lines.wgsl`'s `encoded`. The target was created with it among its
        // view formats, the same view `renderer::gui` composites through.
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview lines"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &depth, &[]);
        for kind in 0..2 {
            for layer in &self.layers {
                let batch = layer.batches()[kind];
                for (on_top, pipeline) in [false, true].into_iter().zip(&gpu.pipelines[kind]) {
                    if let Some((buffer, range)) = batch.range(on_top) {
                        pass.set_pipeline(pipeline);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(range, 0..1);
                    }
                }
            }
        }
    }
}

/// The pipelines for one target format and one scene sample count — lines
/// then dots, each depth-tested then drawn through — and the layout the
/// scene's depth buffer is bound through.
struct Gpu {
    format: wgpu::TextureFormat,
    samples: u32,
    depth_layout: wgpu::BindGroupLayout,
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
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
                depth_stencil: None,
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
        }
    }
}
