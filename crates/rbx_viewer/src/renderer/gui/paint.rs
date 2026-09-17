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
use super::quads::{self, Run};
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
                format,
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

#[cfg(test)]
mod tests {
    use rbx_dom::{Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence};

    use super::*;
    use crate::renderer::texture;
    use crate::scene::{GuiGradient, GuiGradientKind, GuiJoin, GuiRect, GuiStroke, GuiTile};

    // The pipeline has to accept the vertex layout the quads emit, and a
    // rounded, stroked, shaded element has to make it through a whole
    // prepare-and-draw — which is the one thing a CPU-side test cannot say.
    #[test]
    fn a_rounded_stroked_shaded_element_draws_through_the_pipeline() {
        let Some((device, queue)) = crate::gpu::for_tests() else {
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let viewport_layout = pipeline::viewport_layout(&device);
        let image_layout = texture::layout(&device);
        let mut painter = Painter::new(&device, &queue, format, &viewport_layout, &image_layout);

        let element = GuiElement {
            rect: GuiRect {
                x: 8.0,
                y: 8.0,
                width: 48.0,
                height: 32.0,
            },
            clip: None,
            rotation: 30.0,
            background: [1.0, 1.0, 1.0],
            background_alpha: 1.0,
            border: None,
            image: None,
            border_inset: 0.0,
            z_index: 1,
            corner_radii: [8.0; 4],
            stroke: Some(GuiStroke {
                color: [0.0; 3],
                alpha: 1.0,
                band: [0.0, 3.0],
                join: GuiJoin::Round,
                on_text: false,
            }),
            gradient: Some(GuiGradient {
                color: ColorSequence {
                    keypoints: vec![ColorSequenceKeypoint {
                        time: 0.0,
                        color: Color3Data {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                        },
                        envelope: 0.0,
                    }],
                },
                transparency: NumberSequence { keypoints: vec![] },
                origin: [0.0, 0.0],
                axis: [1.0 / 48.0, 0.0],
                kind: GuiGradientKind::Linear,
                tile: GuiTile::Clamp,
            }),
            text: None,
        };
        let size = (64, 48);
        painter.prepare(
            &device,
            &queue,
            &[element],
            &HashMap::new(),
            size,
            &mut Typesetter::new(),
        );

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let white = texture::Uploaded::color(
            &device,
            &queue,
            &crate::assets::Image {
                width: 1,
                height: 1,
                pixels: vec![255; 4],
            },
        );
        let sampler = texture::sampler(&device, wgpu::AddressMode::Repeat, 1);
        let groups = [white.bind(&device, &image_layout, &sampler, 1)];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter.draw(
            &mut encoder,
            &view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &groups,
            size,
        );
        queue.submit(std::iter::once(encoder.finish()));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("the GPU never caught up");
    }
}
