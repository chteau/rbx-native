//! GPU presentation of `crate::scene::beam`: every `Beam`'s ribbon is rebuilt
//! from scratch each frame (`FaceCamera` needs the eye, `TextureSpeed` needs
//! wall-clock time — see [`ribbon`]) and drawn in its own pass, after opaque
//! geometry and before particles (see `Renderer::draw`), depth-tested against
//! the scene but writing no depth of its own.
//!
//! The pipeline, shader and vertex layout live in [`pipeline`]; the CPU-side
//! vertex build lives in [`ribbon`]; this file is the texture bookkeeping and
//! the per-frame build/upload/draw.

mod patch;
mod pipeline;
mod ribbon;

use std::collections::HashMap;
use std::ops::Range;
use std::time::Instant;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;

use super::pipeline::Target;
use super::post::Targets;
use super::texture;
use crate::assets::{self, Image};
use crate::quality::QualityProfile;
use crate::scene::Beam;
use pipeline::{CameraRaw, VertexRaw};

/// One distinct texture's upload, kept alive beside the bind group that views
/// it (see [`texture::Uploaded`]).
struct Slot {
    bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    uploaded: texture::Uploaded,
}

/// One texture group's contiguous slice of the shared vertex buffer.
struct Run {
    texture: usize,
    range: Range<u32>,
}

pub(super) struct Beams {
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    image_layout: wgpu::BindGroupLayout,
    /// Index 0 is always the flat-white fallback both `AssetRef::Empty` and a
    /// texture that failed to download draw through (see [`Beams::new`]) —
    /// Roblox itself falls back to a solid line in both cases.
    textures: Vec<Slot>,
    live: Vec<(Beam, usize)>,
    /// Whether the quality profile draws beams at all — `false` keeps `live`
    /// empty for the whole run, [`Beams::replace`] included.
    enabled: bool,
    /// The slot in `textures` every reference [`Beams::new`] tried resolved
    /// to (0 where the download failed); a reference missing here was never
    /// attempted — see [`Beams::replace`].
    slots: HashMap<AssetRef, usize>,
    vertices: Option<wgpu::Buffer>,
    /// Grown, never shrunk — same reasoning as `renderer::particles::Particles::instances`.
    vertex_capacity: usize,
    last_tick: Option<Instant>,
    /// Wall-clock seconds since the first [`Beams::draw`]; what `TextureSpeed`
    /// scrolls against. `None` for the very first call keeps a `--screenshot`
    /// deterministic (see `renderer::particles::Particles::last_tick`).
    elapsed: f32,
}

impl Beams {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        beams: &[Beam],
        quality: &QualityProfile,
    ) -> Self {
        let camera_layout = pipeline::camera_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview beam camera"),
            size: std::mem::size_of::<CameraRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview beam camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let image_layout = texture::layout(device);
        let render_pipeline =
            pipeline::create_pipeline(device, target, &camera_layout, &image_layout);

        if !quality.beams || beams.is_empty() {
            return Beams {
                pipeline: render_pipeline,
                camera_layout,
                camera_buffer,
                camera_bind_group,
                image_layout,
                textures: Vec::new(),
                live: Vec::new(),
                enabled: quality.beams,
                slots: HashMap::new(),
                vertices: None,
                vertex_capacity: 0,
                last_tick: None,
                elapsed: 0.0,
            };
        }

        // V repeats (`TextureMode`/`TextureSpeed` tiling and scrolling), U
        // does not: a beam's texture never tiles across its width. Roblox
        // samples a beam's length along the texture's row axis (see
        // `ribbon::vertices`), not its column axis.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview beam sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: quality.anisotropy.max(1),
            ..Default::default()
        });

        let mut textures = vec![white_slot(device, queue, &image_layout, &sampler, quality)];
        let references = texture_refs(beams);
        // Live-effect asset warnings aren't wired to the Output dock yet — see
        // `assets::load`'s doc comment; only scene-load-time warnings are.
        let (images, _warnings) = assets::load(&references);
        let mut slots = HashMap::new();
        for reference in references {
            let slot = match images.get(&reference) {
                Some(image) => {
                    let uploaded = texture::Uploaded::color(device, queue, image);
                    let bind_group =
                        uploaded.bind(device, &image_layout, &sampler, quality.texture_max_size);
                    textures.push(Slot {
                        bind_group,
                        uploaded,
                    });
                    textures.len() - 1
                }
                // Never downloaded, or the fetch failed: Roblox itself falls
                // back to a solid line here (see `Beam.Texture`'s docs).
                None => 0,
            };
            slots.insert(reference, slot);
        }

        let live: Vec<(Beam, usize)> = beams
            .iter()
            .map(|beam| {
                let texture = patch::slot_of(&slots, &beam.texture).unwrap_or(0);
                (beam.clone(), texture)
            })
            .collect();

        Beams {
            pipeline: render_pipeline,
            camera_layout,
            camera_buffer,
            camera_bind_group,
            image_layout,
            textures,
            live,
            enabled: true,
            slots,
            vertices: None,
            vertex_capacity: 0,
            last_tick: None,
            elapsed: 0.0,
        }
    }

    /// Rebuilds the pipeline for a new sample count — see `renderer::switch`.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        self.pipeline =
            pipeline::create_pipeline(device, target, &self.camera_layout, &self.image_layout);
    }

    /// Rebuilds every ribbon from `eye`/wall-clock time, uploads them and
    /// draws one run per texture group over `targets`'s existing colour and
    /// depth attachments (loaded, not cleared — see `renderer::particles`,
    /// whose pass this one runs just before).
    pub(super) fn draw(
        &mut self,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        eye: Vec3,
        view_projection: Mat4,
    ) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .replace(now)
            .map_or(0.0, |previous| (now - previous).as_secs_f32().min(0.25));
        self.elapsed += dt;

        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraRaw {
                view_projection: view_projection.to_cols_array_2d(),
            }),
        );

        if self.live.is_empty() {
            return;
        }
        let (vertices, runs) = self.build(eye);
        self.upload(device, queue, &vertices);
        let Some(buffer) = &self.vertices else {
            return;
        };

        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview beams"),
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

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for run in &runs {
            let Some(slot) = self.textures.get(run.texture) else {
                continue;
            };
            pass.set_bind_group(1, &slot.bind_group, &[]);
            pass.draw(run.range.clone(), 0..1);
        }
    }

    /// One ribbon per beam, concatenated into groups by texture (first-seen
    /// order) and bridged within each group with degenerate triangles (see
    /// [`ribbon::append`]) so a whole group draws in a single [`Run`].
    fn build(&self, eye: Vec3) -> (Vec<VertexRaw>, Vec<Run>) {
        let mut order: Vec<usize> = Vec::new();
        for &(_, texture) in &self.live {
            if !order.contains(&texture) {
                order.push(texture);
            }
        }

        let mut vertices = Vec::new();
        let mut runs = Vec::with_capacity(order.len());
        for texture in order {
            let start = vertices.len();
            let mut started = false;
            for (beam, beam_texture) in &self.live {
                if *beam_texture != texture {
                    continue;
                }
                let beam_vertices = ribbon::vertices(beam, eye, self.elapsed);
                if started {
                    ribbon::append(&mut vertices, &beam_vertices);
                } else {
                    vertices.extend(beam_vertices);
                    started = true;
                }
            }
            runs.push(Run {
                texture,
                range: start as u32..vertices.len() as u32,
            });
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
                label: Some("rbxview beam vertices"),
                size: (self.vertex_capacity * std::mem::size_of::<VertexRaw>())
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(buffer) = &self.vertices else {
            return;
        };
        queue.write_buffer(buffer, 0, bytemuck::cast_slice(vertices));
    }
}

/// A 1x1 white texture: what `AssetRef::Empty` and a download failure both
/// draw through, so a beam with no texture is a flat-coloured ribbon rather
/// than needing a second, textureless shader path.
fn white_slot(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    quality: &QualityProfile,
) -> Slot {
    let image = Image {
        width: 1,
        height: 1,
        pixels: vec![255, 255, 255, 255],
    };
    let uploaded = texture::Uploaded::color(device, queue, &image);
    let bind_group = uploaded.bind(device, image_layout, sampler, quality.texture_max_size);
    Slot {
        bind_group,
        uploaded,
    }
}

/// Every distinct non-empty texture the given beams need, in first-seen
/// order — kept stable so re-running the same file draws the same
/// texture-to-slot mapping.
fn texture_refs(beams: &[Beam]) -> Vec<AssetRef> {
    let mut seen = Vec::new();
    for beam in beams {
        if beam.texture != AssetRef::Empty && !seen.contains(&beam.texture) {
            seen.push(beam.texture.clone());
        }
    }
    seen
}
