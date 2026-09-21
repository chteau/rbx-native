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

use super::outline::{edges, Vertex};
use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("preview.wgsl");

pub(super) struct Preview {
    pipeline: wgpu::RenderPipeline,
    vertices: Option<wgpu::Buffer>,
    count: u32,
}

impl Preview {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let pipeline = pipeline::surface(
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
        );

        Preview {
            pipeline,
            vertices: None,
            count: 0,
        }
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
