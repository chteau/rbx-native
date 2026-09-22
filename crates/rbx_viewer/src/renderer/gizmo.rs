//! The transform gizmo, drawn as solid geometry over the scene.
//!
//! One unlit, vertex-coloured triangle pipeline and the small vertex buffer
//! [`mesh`] rewrites each frame — never occluded, because a handle is a
//! control rather than scenery.

use glam::Vec3;

use crate::gizmo::{End, Shape};

use super::pipeline::{self, Surface, Target};
use mesh::{Vertex, CAPACITY};

mod mesh;

const SHADER: &str = include_str!("gizmo.wgsl");

/// The layout [`Draggers::triangles`] is laid out in.
pub(super) fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    Vertex::layout()
}

/// The gizmo's GPU state.
pub(super) struct Draggers {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    count: u32,
}

impl Draggers {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let pipeline = pipeline::surface(
            device,
            target,
            &Surface {
                // Never occluded: a handle is a control, not scenery, and one
                // buried inside the part it moves would be impossible to
                // grab. `translucent` here is only borrowed for its second
                // effect — leaving the depth buffer alone, which the
                // depth-of-field and fog passes downstream still read as the
                // real scene's.
                compare: wgpu::CompareFunction::Always,
                translucent: true,
                ..Surface::new(
                    "rbxview gizmo",
                    SHADER,
                    &[Some(frame_layout)],
                    &[Some(Vertex::layout())],
                )
            },
        );

        Draggers {
            pipeline,
            vertices: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview gizmo"),
                size: (CAPACITY * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            count: 0,
        }
    }

    /// Rebuilds this frame's handles, or draws none at all when nothing is
    /// selected or no transform tool is active.
    pub(super) fn update(
        &mut self,
        queue: &wgpu::Queue,
        shape: Option<Shape>,
        held: Option<End>,
        eye: Vec3,
    ) {
        let Some(shape) = shape else {
            self.count = 0;
            return;
        };

        let vertices = mesh::mesh(&shape, held, eye);
        self.count = vertices.len() as u32;
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
    }

    /// This frame's handle triangles, `None` with none drawn — what the
    /// line pass keeps its guides off (see `renderer::lines`).
    pub(super) fn triangles(&self) -> Option<(&wgpu::Buffer, u32)> {
        (self.count > 0).then_some((&self.vertices, self.count))
    }

    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        if self.count == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}
