//! Plain world-space line segments an editor draws over the scene — Studio's
//! light guides today. Nothing here knows what a segment means: the
//! embedder works out where each one goes and hands the lot over, the way
//! `renderer::preview` takes bare boxes.
//!
//! Drawn with the adornment pass's own line shader and pipelines (a
//! `LineHandleAdornment` is the same thing: a world-space segment held at a
//! constant width on screen), so the two cannot drift apart. Costs nothing
//! until the first segment arrives — the pipelines are built then, not
//! before — and draws nothing once the list is cleared again.

use glam::Vec3;

use super::adornment::{line, line_pipelines, Batch};
use super::pipeline::Target;

/// Every segment's width on screen. Measured off Studio's own light guides,
/// whose lines are two pixels across at 100% scale.
const PIXELS: f32 = 2.0;

/// One line an editor overlay draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub from: Vec3,
    pub to: Vec3,
    /// Linear RGB, and alpha.
    pub color: [f32; 4],
    /// Drawn through whatever stands in front of it rather than hidden by it.
    pub on_top: bool,
}

#[derive(Default)]
pub(super) struct Lines {
    pipelines: Option<[wgpu::RenderPipeline; 2]>,
    batch: Batch,
}

impl Lines {
    /// Replaces every segment. An empty list clears them.
    pub(super) fn set(&mut self, device: &wgpu::Device, target: Target, segments: &[Segment]) {
        let mut sides = [Vec::new(), Vec::new()];
        for segment in segments {
            line(
                segment.from,
                segment.to,
                PIXELS,
                segment.color,
                &mut sides[usize::from(segment.on_top)],
            );
        }
        let [occluded, on_top] = sides;
        self.batch = Batch::build(device, "rbxview lines", &occluded, &on_top);
        if self.pipelines.is_none() && !segments.is_empty() {
            self.pipelines = Some(line_pipelines(device, target));
        }
    }

    /// Follows a new sample count — see
    /// `renderer::switch::Renderer::rebuild_pipelines`.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        if self.pipelines.is_some() {
            self.pipelines = Some(line_pipelines(device, target));
        }
    }

    /// The depth-tested segments first, then the ones drawn through.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let Some(pipelines) = &self.pipelines else {
            return;
        };
        for (on_top, pipeline) in [false, true].into_iter().zip(pipelines) {
            if let Some((buffer, range)) = self.batch.range(on_top) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, frame, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range, 0..1);
            }
        }
    }
}
