//! GPU presentation of `crate::scene::trail`: unlike a `Beam`'s ribbon, a
//! trail's shape is not rebuilt from a static definition every frame — it is
//! *accumulated*, so this is where each live `Trail` keeps its own
//! [`crate::scene::TrailRecorder`] between frames and feeds it
//! `Attachment0`/`Attachment1`'s current world position every [`Trails::draw`]
//! call, drawn in its own pass right after beams and before particles (see
//! `Renderer::draw`), depth-tested against the scene but writing no depth of
//! its own.
//!
//! This viewer never moves an `Attachment` between frames (see
//! `crate::scene::trail`'s module doc), so every `Recorder` here only ever
//! holds a single sample and [`ribbon::vertices`] always returns empty: a
//! documented, correct no-op today, exercised for real by
//! `renderer::trail::ribbon`'s own tests, which replay a scripted motion path
//! through the identical code.
//!
//! The pipeline, shader and vertex layout live in [`pipeline`]; the CPU-side
//! vertex build lives in [`ribbon`]; this file is the texture bookkeeping,
//! the running recorders and the per-frame build/upload/draw.

mod pipeline;
mod ribbon;

use std::ops::Range;
use std::time::Instant;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;

use super::pipeline::Target;
use super::post::Targets;
use super::texture;
use crate::assets::{self, Image};
use crate::quality::QualityProfile;
use crate::scene::{Trail, TrailRecorder};
use pipeline::{CameraRaw, VertexRaw};

/// One distinct texture's upload, kept alive beside the bind group that views
/// it — identical role to `renderer::beam::Slot`.
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

pub(super) struct Trails {
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    image_layout: wgpu::BindGroupLayout,
    /// Index 0 is always the flat-white fallback, both for `AssetRef::Empty`
    /// and a texture that failed to download — see `renderer::beam::Beams`'s
    /// identical field.
    textures: Vec<Slot>,
    /// Each live trail's static definition, paired with the running history
    /// only `Trails` (not `crate::scene::Scene`) has any business owning —
    /// see this module's doc.
    live: Vec<(Trail, TrailRecorder, usize)>,
    vertices: Option<wgpu::Buffer>,
    /// Grown, never shrunk — same reasoning as `renderer::beam::Beams::vertex_capacity`.
    vertex_capacity: usize,
    last_tick: Option<Instant>,
    /// Wall-clock seconds since the first [`Trails::draw`]; what every
    /// `Recorder` times its samples against. `None` for the very first call
    /// keeps a `--screenshot` deterministic — see
    /// `renderer::beam::Beams::elapsed`.
    elapsed: f32,
}

impl Trails {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        trails: &[Trail],
        quality: &QualityProfile,
    ) -> Self {
        let camera_layout = pipeline::camera_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview trail camera"),
            size: std::mem::size_of::<CameraRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview trail camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let image_layout = texture::layout(device);
        let render_pipeline =
            pipeline::create_pipeline(device, target, &camera_layout, &image_layout);

        if !quality.trails || trails.is_empty() {
            return Trails {
                pipeline: render_pipeline,
                camera_layout,
                camera_buffer,
                camera_bind_group,
                image_layout,
                textures: Vec::new(),
                live: Vec::new(),
                vertices: None,
                vertex_capacity: 0,
                last_tick: None,
                elapsed: 0.0,
            };
        }

        // U repeats by `TextureLength`, V does not: a trail's texture never
        // tiles across its width — same sampler settings as `renderer::beam`.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview trail sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: quality.anisotropy.max(1),
            ..Default::default()
        });

        let mut textures = vec![white_slot(device, queue, &image_layout, &sampler, quality)];
        let references = texture_refs(trails);
        let images = assets::load(&references);
        let mut slot_of: Vec<usize> = Vec::with_capacity(references.len());
        for reference in &references {
            let slot = match images.get(reference) {
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
                // back to a solid plane here (see `Trail.Texture`'s docs).
                None => 0,
            };
            slot_of.push(slot);
        }

        let live: Vec<(Trail, TrailRecorder, usize)> = trails
            .iter()
            .map(|trail| {
                let texture = if trail.texture == AssetRef::Empty {
                    0
                } else {
                    references
                        .iter()
                        .position(|reference| *reference == trail.texture)
                        .map(|index| slot_of[index])
                        .unwrap_or(0)
                };
                (trail.clone(), TrailRecorder::new(), texture)
            })
            .collect();

        Trails {
            pipeline: render_pipeline,
            camera_layout,
            camera_buffer,
            camera_bind_group,
            image_layout,
            textures,
            live,
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

    /// Advances every trail's recorder by one frame, rebuilds every ribbon,
    /// uploads them and draws one run per texture group over `targets`'s
    /// existing colour and depth attachments — same pass shape as
    /// `renderer::beam::Beams::draw`, run right after it.
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

        for (trail, recorder, _) in &mut self.live {
            // `Enabled = false` stops new segments but not existing ones
            // ageing out — see `crate::scene::Trail::enabled`'s doc — so
            // `expire` always runs, `record` only when enabled.
            if trail.enabled {
                recorder.record(
                    self.elapsed,
                    trail.position0,
                    trail.position1,
                    trail.min_length,
                );
            }
            recorder.expire(self.elapsed, trail.lifetime);
        }

        let (vertices, runs) = self.build(eye);
        self.upload(device, queue, &vertices);
        let Some(buffer) = &self.vertices else {
            return;
        };
        if runs.is_empty() {
            return;
        }

        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview trails"),
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

    /// One ribbon per trail with at least two recorded samples, concatenated
    /// into groups by texture (first-seen order) and bridged within each
    /// group with degenerate triangles — identical grouping to
    /// `renderer::beam::Beams::build`.
    fn build(&self, eye: Vec3) -> (Vec<VertexRaw>, Vec<Run>) {
        let mut order: Vec<usize> = Vec::new();
        for &(_, _, texture) in &self.live {
            if !order.contains(&texture) {
                order.push(texture);
            }
        }

        let mut vertices = Vec::new();
        let mut runs = Vec::with_capacity(order.len());
        for texture in order {
            let start = vertices.len();
            let mut started = false;
            for (trail, recorder, trail_texture) in &self.live {
                if *trail_texture != texture {
                    continue;
                }
                let trail_vertices = ribbon::vertices(trail, recorder, eye, self.elapsed);
                if trail_vertices.is_empty() {
                    continue;
                }
                if started {
                    ribbon::append(&mut vertices, &trail_vertices);
                } else {
                    vertices.extend(trail_vertices);
                    started = true;
                }
            }
            if vertices.len() > start {
                runs.push(Run {
                    texture,
                    range: start as u32..vertices.len() as u32,
                });
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
                label: Some("rbxview trail vertices"),
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
/// draw through — identical role to `renderer::beam::white_slot`.
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

/// Every distinct non-empty texture the given trails need, in first-seen
/// order — kept stable so re-running the same file draws the same
/// texture-to-slot mapping.
fn texture_refs(trails: &[Trail]) -> Vec<AssetRef> {
    let mut seen = Vec::new();
    for trail in trails {
        if trail.texture != AssetRef::Empty && !seen.contains(&trail.texture) {
            seen.push(trail.texture.clone());
        }
    }
    seen
}
