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
//! on screen. The transform handles are stamped into a stencil first and
//! every guide kept off them, since Studio draws its handles over its guides.
//! Costs nothing until the first segment arrives — the pipelines are built
//! then, not before — and draws nothing once the list is cleared again.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::adornment::{line, Batch};
use super::post::Targets;
use gpu::{Gpu, HANDLE};

mod gpu;

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
    /// The handle stencil, and the target size it was made for.
    stencil: Option<(wgpu::Extent3d, wgpu::TextureView)>,
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
    /// layer's dots, and none of them over `handles`, the transform
    /// handles' triangles this frame (see `renderer::gizmo::Draggers`).
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        scene: &Targets,
        frame: &wgpu::BindGroup,
        handles: Option<(&wgpu::Buffer, u32)>,
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
        let size = target.size();
        if self
            .stencil
            .as_ref()
            .is_none_or(|(made_for, _)| *made_for != size)
        {
            self.stencil = Some((size, gpu::stencil_target(device, size)));
        }
        let (Some(gpu), Some((_, stencil))) = (&self.gpu, &self.stencil) else {
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
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: stencil,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &depth, &[]);
        pass.set_stencil_reference(HANDLE);
        if let Some((vertices, count)) = handles {
            pass.set_pipeline(&gpu.handles);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..count, 0..1);
        }
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
