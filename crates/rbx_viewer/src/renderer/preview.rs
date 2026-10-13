//! The ghost boxes an editor draws to show where something *would* land —
//! today the Align tool's live preview, which `studio/align-tool.md`
//! describes as "dynamically previewing the point of alignment before
//! confirming".
//!
//! Box edges like [`super::selection`]'s and [`super::hover`]'s, in a
//! colour of their own, but built from plain model matrices rather than
//! from anything in the scene: the whole point is to draw a placement no
//! instance has yet. Nothing here knows what the boxes mean, which is what
//! keeps the next tool that needs one from having to add a second pass.

use glam::Mat4;
use wgpu::util::DeviceExt;

use super::cue::{overlay_pipeline, OverlayColors};
use super::outline::{edges, Vertex};
use super::pipeline::{Surface, Target};

const SHADER: &str = include_str!("preview.wgsl");

pub(super) struct Preview {
    pipeline: wgpu::RenderPipeline,
    /// The colour the pipeline was built with, and what it needs to be built
    /// again when it changes.
    color: [f32; 4],
    target: Target,
    frame_layout: wgpu::BindGroupLayout,
    vertices: Option<wgpu::Buffer>,
    count: u32,
}

impl Preview {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let color = OverlayColors::default().preview;

        Preview {
            pipeline: Self::pipeline(device, target, frame_layout, color),
            color,
            target,
            frame_layout: frame_layout.clone(),
            vertices: None,
            count: 0,
        }
    }

    fn pipeline(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        color: [f32; 4],
    ) -> wgpu::RenderPipeline {
        overlay_pipeline(
            device,
            target,
            &Surface {
                cull: None,
                // Never occluded, like the draggers: a preview is something
                // the user is deciding about, and one buried inside the
                // geometry it is being compared against would be no help.
                compare: wgpu::CompareFunction::Always,
                translucent: true,
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Surface::new(
                    "rbxview preview",
                    SHADER,
                    &[Some(frame_layout)],
                    &[Some(Vertex::layout())],
                )
            },
            color,
        )
    }

    /// Redraws the ghost boxes in `colors.preview`, rebuilding the pipeline
    /// only when that differs from the colour already in use.
    pub(super) fn set_overlay_colors(&mut self, device: &wgpu::Device, colors: OverlayColors) {
        if self.color == colors.preview {
            return;
        }
        self.color = colors.preview;
        self.pipeline = Self::pipeline(device, self.target, &self.frame_layout, self.color);
    }

    /// Replaces the ghost boxes. An empty list clears them, which is what
    /// closing the tool that asked for them sends.
    pub(super) fn set(&mut self, device: &wgpu::Device, boxes: &[Mat4]) {
        let vertices: Vec<Vertex> = boxes.iter().copied().flat_map(edges).collect();
        self.count = vertices.len() as u32;
        self.vertices = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview preview"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }

    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let Some(vertices) = &self.vertices else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}
