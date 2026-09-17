//! Painting a resolved GUI tree onto *a* target of *a* pixel size.
//!
//! The screen overlay and a `BillboardGui`/`SurfaceGui` canvas differ only in
//! those two arguments and in whether the target is loaded or cleared first,
//! so both go through this one painter rather than through two copies of the
//! same quad build and the same render pass.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::atlas::Slot;
use super::pipeline::{self, VertexRaw, ViewportRaw};
use super::quads::{self, Run};
use crate::scene::GuiElement;

pub(super) struct Painter {
    pipeline: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    /// Grown, never shrunk — same reasoning as `renderer::trail::Trails`.
    vertex_capacity: usize,
    runs: Vec<Run>,
}

impl Painter {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_layout: &wgpu::BindGroupLayout,
        image_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let viewport_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview gui viewport"),
            size: std::mem::size_of::<ViewportRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview gui viewport"),
            layout: viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        Painter {
            pipeline: pipeline::create_pipeline(device, format, viewport_layout, image_layout),
            viewport_buffer,
            viewport_bind_group,
            vertices: None,
            vertex_capacity: 0,
            runs: Vec::new(),
        }
    }

    /// Turns `elements` into the quads a target of `size` pixels wants, and
    /// uploads them. Must not run inside a render pass: it writes the very
    /// buffers [`Painter::draw`] reads.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        elements: &[GuiElement],
        slot_of: &HashMap<AssetRef, Slot>,
        size: (u32, u32),
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportRaw {
                size: [size.0 as f32, size.1 as f32],
                padding: [0.0, 0.0],
            }),
        );

        let (vertices, runs) = quads::build(elements, slot_of, size);
        self.runs = runs;
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len();
            self.vertices = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview gui vertices"),
                size: (self.vertex_capacity * std::mem::size_of::<VertexRaw>())
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.vertices {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
        }
    }

    /// Draws whatever [`Painter::prepare`] last built over `target`, in one
    /// pass. `load` is what separates the two callers: the screen overlay
    /// composites over a finished frame, a canvas starts from nothing.
    ///
    /// The pass is begun even with nothing to draw, so that a `load` of
    /// `Clear` still leaves the target defined.
    pub(super) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        textures: &[wgpu::BindGroup],
        size: (u32, u32),
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview gui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        let Some(buffer) = &self.vertices else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for run in &self.runs {
            let Some(group) = textures.get(run.texture) else {
                continue;
            };
            match run.scissor {
                Some(scissor) => {
                    pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height)
                }
                // Back to the whole target: a scissor set for one run would
                // otherwise still be in force for the next.
                None => pass.set_scissor_rect(0, 0, size.0, size.1),
            }
            pass.set_bind_group(1, group, &[]);
            pass.draw(run.range.clone(), 0..1);
        }
    }
}
