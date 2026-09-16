//! `BillboardGui`/`SurfaceGui` presentation: each container's tree is painted
//! once into an offscreen canvas by the very same painter the screen overlay
//! uses, and that canvas is then drawn as a textured quad inside the scene.
//!
//! Baking happens in [`Space::new`] rather than per frame because nothing in
//! one of these trees moves; what does move is a billboard's quad, which needs
//! the eye and so is rebuilt every frame (see [`quad`]).

mod pipeline;
mod quad;

use std::ops::Range;

use glam::{Mat4, Vec3};

use super::atlas::Atlas;
use super::paint::Painter;
use crate::renderer::pipeline::Target;
use crate::renderer::post::Targets;
use crate::renderer::texture;
use crate::scene::{gui_canvas_layout, GuiAnchor, SpaceGui};
use pipeline::{CameraRaw, VertexRaw};

/// Canvases are painted in this format rather than in the display's: the
/// in-world pass samples them into a linear HDR target, so the sRGB decode has
/// to be guaranteed whatever surface format the window happens to hand us.
const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One baked canvas, kept alive beside the bind group that views it.
struct Canvas {
    bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    texture: wgpu::Texture,
}

struct Item {
    anchor: GuiAnchor,
    always_on_top: bool,
    canvas: usize,
}

pub(super) struct Space {
    depth_tested: wgpu::RenderPipeline,
    always_on_top: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    image_layout: wgpu::BindGroupLayout,
    /// The painter every canvas is baked with, built for [`CANVAS_FORMAT`].
    /// `None` until the first scene with a canvas to bake: a place without
    /// one never pays for its pipeline, and one that has some keeps it across
    /// every rebuild rather than compiling it again per reload.
    painter: Option<Painter>,
    canvases: Vec<Canvas>,
    items: Vec<Item>,
    vertices: Option<wgpu::Buffer>,
    /// Grown, never shrunk — same reasoning as `renderer::beam::Beams`.
    vertex_capacity: usize,
}

impl Space {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        viewport_layout: &wgpu::BindGroupLayout,
        atlas: &Atlas,
        spaces: &[SpaceGui],
    ) -> Self {
        let camera_layout = pipeline::camera_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview gui space camera"),
            size: std::mem::size_of::<CameraRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview gui space camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let image_layout = texture::layout(device);
        let (depth_tested, always_on_top) =
            pipeline::create_pipelines(device, target, &camera_layout, &image_layout);

        let mut space = Space {
            depth_tested,
            always_on_top,
            camera_layout,
            camera_buffer,
            camera_bind_group,
            image_layout,
            painter: None,
            canvases: Vec::new(),
            items: Vec::new(),
            vertices: None,
            vertex_capacity: 0,
        };
        space.rebuild(device, queue, viewport_layout, atlas, spaces);
        space
    }

    /// Bakes `spaces`' canvases afresh, keeping the pipelines and the painter.
    /// Nothing else is worth keeping: a canvas is the painted tree, and the
    /// tree is what a scene rebuild is for.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_layout: &wgpu::BindGroupLayout,
        atlas: &Atlas,
        spaces: &[SpaceGui],
    ) {
        self.canvases.clear();
        self.items.clear();
        if spaces.is_empty() {
            return;
        }

        // Clamped, unlike the atlas' own sampler: a canvas is sampled over
        // exactly its 0..1 rectangle, so a repeat mode could only ever bleed
        // the opposite edge in along a seam.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview gui canvas"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut painter = self.painter.take().unwrap_or_else(|| {
            Painter::new(device, CANVAS_FORMAT, viewport_layout, &atlas.image_layout)
        });
        for gui in spaces {
            let canvas = bake(device, queue, &mut painter, atlas, gui);
            self.canvases.push(Canvas {
                bind_group: self.bind(device, &canvas, &sampler),
                texture: canvas,
            });
            self.items.push(Item {
                anchor: gui.anchor,
                always_on_top: gui.always_on_top,
                canvas: self.canvases.len() - 1,
            });
        }
        self.painter = Some(painter);
    }

    /// Rebuilds both pipelines for a new sample count — see `renderer::switch`.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        let (depth_tested, always_on_top) =
            pipeline::create_pipelines(device, target, &self.camera_layout, &self.image_layout);
        self.depth_tested = depth_tested;
        self.always_on_top = always_on_top;
    }

    /// Rebuilds every quad from `eye` and draws them over `targets`'s existing
    /// colour and depth attachments, `AlwaysOnTop` canvases last so they land
    /// over the depth-tested ones as well as over the scene.
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        eye: Vec3,
        view_projection: Mat4,
    ) {
        if self.items.is_empty() {
            return;
        }
        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraRaw {
                view_projection: view_projection.to_cols_array_2d(),
            }),
        );

        let (vertices, runs) = self.build(eye);
        self.upload(device, queue, &vertices);
        let Some(buffer) = &self.vertices else {
            return;
        };

        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview gui space"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: targets.depth(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (canvas, always_on_top, range) in runs {
            let Some(canvas) = self.canvases.get(canvas) else {
                continue;
            };
            pass.set_pipeline(match always_on_top {
                true => &self.always_on_top,
                false => &self.depth_tested,
            });
            pass.set_bind_group(1, &canvas.bind_group, &[]);
            pass.draw(range, 0..1);
        }
    }

    /// Every quad, depth-tested ones first. `AlwaysOnTop` is a paint order as
    /// much as a depth mode: a canvas that ignores the depth buffer still has
    /// to land over one that does not.
    #[allow(clippy::type_complexity)]
    fn build(&self, eye: Vec3) -> (Vec<VertexRaw>, Vec<(usize, bool, Range<u32>)>) {
        let mut vertices = Vec::new();
        let mut runs = Vec::new();
        for always_on_top in [false, true] {
            for item in self
                .items
                .iter()
                .filter(|item| item.always_on_top == always_on_top)
            {
                let start = vertices.len() as u32;
                quad::vertices(quad::corners(&item.anchor, eye), &mut vertices);
                runs.push((item.canvas, always_on_top, start..vertices.len() as u32));
            }
        }
        (vertices, runs)
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vertices: &[VertexRaw]) {
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len();
            self.vertices = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview gui space vertices"),
                size: (self.vertex_capacity * std::mem::size_of::<VertexRaw>())
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.vertices {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(vertices));
        }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview gui canvas"),
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

/// Paints one container's tree into a texture of its own `CanvasSize`.
///
/// Submitted on the spot rather than folded into the frame encoder: the canvas
/// is a one-off, and `Renderer::new` has no encoder of its own to borrow.
fn bake(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut Painter,
    atlas: &Atlas,
    gui: &SpaceGui,
) -> wgpu::Texture {
    let size = (gui.canvas[0].max(1.0) as u32, gui.canvas[1].max(1.0) as u32);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview gui canvas"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CANVAS_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    painter.prepare(
        device,
        queue,
        &gui_canvas_layout(gui),
        atlas.slot_of(),
        size,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rbxview gui canvas"),
    });
    // Cleared to fully transparent, not to a colour: anything the tree leaves
    // uncovered must show the scene behind the canvas.
    painter.draw(
        &mut encoder,
        &view,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        atlas.groups(),
        size,
    );
    queue.submit(std::iter::once(encoder.finish()));
    texture
}
