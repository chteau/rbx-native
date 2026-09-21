//! Plain world-space line segments an editor draws over the scene — Studio's
//! light guides and dragger guides. Nothing here knows what a segment means:
//! the embedder works out where each one goes and hands the lot over, the
//! way `renderer::preview` takes bare boxes.
//!
//! Drawn with the adornment pass's own line shader and pipelines (a
//! `LineHandleAdornment` is the same thing: a world-space segment held at a
//! constant width on screen), so the two cannot drift apart. A segment whose
//! two ends coincide is a dot instead — Studio's `SphereHandleAdornment`
//! markers, which its draggers size to stay constant on screen — drawn as a
//! round disc that many pixels across by a shader of its own. Costs nothing
//! until the first segment arrives — the pipelines are built then, not
//! before — and draws nothing once the list is cleared again.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::adornment::{self, line, line_pipelines, Batch};
use super::pipeline::{self, Target};

const DOT_SHADER: &str = include_str!("lines_dot.wgsl");

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
    pipelines: Option<[wgpu::RenderPipeline; 2]>,
    dot_pipelines: Option<[wgpu::RenderPipeline; 2]>,
    /// Each layer's lines and dots, uploaded apart so that replacing one
    /// layer leaves the others' buffers alone.
    layers: Vec<Layer>,
}

#[derive(Default)]
struct Layer {
    lines: Batch,
    dots: Batch,
}

impl Lines {
    /// Replaces one layer's segments. An empty list clears it.
    pub(super) fn set(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        layer: usize,
        segments: &[Segment],
    ) {
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
        if !segments.is_empty() && self.pipelines.is_none() {
            self.build_pipelines(device, target);
        }
    }

    fn build_pipelines(&mut self, device: &wgpu::Device, target: Target) {
        self.pipelines = Some(line_pipelines(device, target));
        let frame = pipeline::frame_layout(device);
        self.dot_pipelines = Some([false, true].map(|on_top| {
            adornment::pipeline(
                device,
                target,
                "rbxview dots",
                DOT_SHADER,
                DotVertex::layout(),
                &[Some(&frame)],
                on_top,
            )
        }));
    }

    /// Follows a new sample count — see
    /// `renderer::switch::Renderer::rebuild_pipelines`.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        if self.pipelines.is_some() {
            self.build_pipelines(device, target);
        }
    }

    /// The depth-tested segments first, then the ones drawn through; the
    /// dots last, over the lines they mark — every layer's lines before any
    /// layer's dots.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let (Some(lines), Some(dots)) = (&self.pipelines, &self.dot_pipelines) else {
            return;
        };
        let batches = self
            .layers
            .iter()
            .map(|layer| (&layer.lines, lines))
            .chain(self.layers.iter().map(|layer| (&layer.dots, dots)));
        for (batch, pipelines) in batches {
            for (on_top, pipeline) in [false, true].into_iter().zip(pipelines) {
                if let Some((buffer, range)) = batch.range(on_top) {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, frame, &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(range, 0..1);
                }
            }
        }
    }
}
