//! Painting a resolved GUI tree onto *a* target of *a* pixel size.
//!
//! The screen overlay and a `BillboardGui`/`SurfaceGui` canvas differ only in
//! those two arguments and in whether the target is loaded or cleared first,
//! so both go through this one painter rather than through two copies of the
//! same quad build and the same render pass.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::atlas::Slot;
use super::gradient::Table;
use super::pipeline::{self, VertexRaw, ViewportRaw};
use super::quads::{self, Run, Scissor};
use super::text::Typesetter;
use crate::scene::GuiElement;

pub(super) struct Painter {
    pipeline: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    /// Grown, never shrunk — same reasoning as `renderer::trail::Trails`.
    vertex_capacity: usize,
    runs: Vec<Run>,
    /// Bind group 2: the `UIGradient` ramps the last build baked.
    gradients: Table,
}

impl Painter {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        viewport_layout: &wgpu::BindGroupLayout,
        image_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let gradients = Table::new(device, queue);
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
            pipeline: pipeline::create_pipeline(
                device,
                // Not `format` itself: the pass is attached through a
                // non-sRGB view of it so the blend happens on encoded values
                // (see `pipeline::encoded`).
                pipeline::encoded(format),
                viewport_layout,
                image_layout,
                &gradients.layout,
            ),
            viewport_buffer,
            viewport_bind_group,
            vertices: None,
            vertex_capacity: 0,
            runs: Vec::new(),
            gradients,
        }
    }

    /// Turns `elements` into the quads a target of `size` pixels wants, and
    /// uploads them. Must not run inside a render pass: it writes the very
    /// buffers [`Painter::draw`] reads.
    ///
    /// The glyphs the text quads sample land in `fonts`' atlas, which the
    /// caller uploads afterwards (see `Atlas::sync_glyphs`).
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        elements: &[GuiElement],
        slot_of: &HashMap<AssetRef, Slot>,
        size: (u32, u32),
        fonts: &mut Typesetter,
    ) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportRaw {
                size: [size.0 as f32, size.1 as f32],
                padding: [0.0, 0.0],
            }),
        );

        let (vertices, runs, rows) = quads::build(elements, slot_of, size, fonts);
        self.runs = runs;
        self.gradients.upload(device, queue, &rows);
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
        self.draw_scaled(encoder, target, load, textures, (size, size));
    }

    /// [`Painter::draw`] for a layout made at `layout` pixels onto a target
    /// `target` pixels across — a screen emulated in a smaller view. The
    /// vertices need nothing: [`Painter::prepare`]'s viewport maps `layout`
    /// onto the whole target whatever its size. The clip rectangles are in
    /// the layout's pixels, and are taken to the target's here.
    pub(super) fn draw_scaled(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        textures: &[wgpu::BindGroup],
        (layout, size): ((u32, u32), (u32, u32)),
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
        pass.set_bind_group(2, self.gradients.bind_group(), &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for run in &self.runs {
            let Some(group) = textures.get(run.texture) else {
                continue;
            };
            match run.scissor.map(|scissor| scissor_on(scissor, layout, size)) {
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

/// `scissor`, cut in a `layout`-pixel layout, on a `target`-pixel target:
/// scaled, widened outwards to whole pixels so no clipped edge loses a row,
/// and kept inside the target, which wgpu requires of a scissor.
fn scissor_on(scissor: Scissor, layout: (u32, u32), target: (u32, u32)) -> Scissor {
    if layout == target {
        return scissor;
    }
    let scale = |value: u32, from: u32, to: u32| value as f64 * to as f64 / from.max(1) as f64;
    let x0 = scale(scissor.x, layout.0, target.0).floor() as u32;
    let y0 = scale(scissor.y, layout.1, target.1).floor() as u32;
    let x1 = (scale(scissor.x + scissor.width, layout.0, target.0).ceil() as u32).min(target.0);
    let y1 = (scale(scissor.y + scissor.height, layout.1, target.1).ceil() as u32).min(target.1);
    Scissor {
        x: x0.min(x1),
        y: y0.min(y1),
        width: x1.saturating_sub(x0),
        height: y1.saturating_sub(y0),
    }
}

#[cfg(test)]
mod tests;
