//! GPU presentation of `crate::scene::particles`: one CPU [`Simulation`] per
//! emitter, advanced with wall-clock `dt` and drawn as camera-facing billboards
//! in their own pass, after opaque geometry and depth-tested against it.
//!
//! The pipeline, shader and vertex layouts live in [`pipeline`]; this file is
//! the emitter bookkeeping and the per-frame simulate/sort/upload/draw.

mod patch;
mod pipeline;

use std::collections::HashMap;
use std::time::Instant;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::pipeline::Target;
use super::post::Targets;
use super::rebuild::untried;
use super::texture;
use crate::assets;
use crate::quality::QualityProfile;
use crate::scene::{Emitter, Simulation};
use pipeline::{CameraRaw, ParticleRaw, QUAD_CORNERS};

/// One [`Emitter`]'s live GPU state: its own simulation, and which uploaded
/// image it draws through.
struct Live {
    emitter: Emitter,
    simulation: Simulation,
    texture: usize,
}

/// One distinct texture's upload, kept alive beside the bind group that views
/// it (see [`texture::Uploaded`]).
struct Slot {
    bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    uploaded: texture::Uploaded,
}

/// One particle already placed in the sorted draw order, with the texture slot
/// its run belongs to.
struct Item {
    texture: usize,
    distance: f32,
    raw: ParticleRaw,
}

pub(super) struct Particles {
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    image_layout: wgpu::BindGroupLayout,
    textures: Vec<Slot>,
    live: Vec<Live>,
    /// Whether the quality profile draws particles at all — `false` keeps
    /// `live` empty for the whole run, [`Particles::replace`] included.
    enabled: bool,
    /// What [`Particles::rebuild`] learned about every texture it was asked
    /// for: `Some(slot)` uploaded into `textures`, `None` tried and failed. A
    /// reference missing here was never attempted, which is the one case
    /// [`Particles::replace`] cannot serve without a download.
    slots: HashMap<AssetRef, Option<usize>>,
    quad: wgpu::Buffer,
    /// Grown, never shrunk: a place whose emitters settle at a smaller steady
    /// state after a burst does not need the buffer downsized again.
    instances: Option<wgpu::Buffer>,
    instance_capacity: usize,
    /// `None` until the first [`Particles::draw`]: that call's `dt` would
    /// otherwise be however long GPU setup took, which is not a simulation
    /// step and would make a `--screenshot` non-deterministic.
    last_tick: Option<Instant>,
}

impl Particles {
    /// Builds the GPU pipeline, then downloads every emitter's texture
    /// (bypassing the scene's own asset plan — this pass owns its own network
    /// call) and pre-warms each emitter to a steady state — see
    /// [`Particles::rebuild`], which is the whole of the second half.
    ///
    /// An emitter whose texture never resolves is dropped outright: there is no
    /// bare fallback for a particle, unlike a `Decal` with no image.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        emitters: &[Emitter],
        quality: &QualityProfile,
    ) -> Self {
        let camera_layout = pipeline::camera_layout(device);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview particle camera"),
            size: std::mem::size_of::<CameraRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview particle camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let image_layout = texture::layout(device);
        let quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview particle quad"),
            contents: bytemuck::cast_slice(&QUAD_CORNERS),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let render_pipeline =
            pipeline::create_pipeline(device, target, &camera_layout, &image_layout);

        let mut particles = Particles {
            pipeline: render_pipeline,
            camera_layout,
            camera_buffer,
            camera_bind_group,
            image_layout,
            textures: Vec::new(),
            live: Vec::new(),
            enabled: quality.particles,
            slots: HashMap::new(),
            quad,
            instances: None,
            instance_capacity: 0,
            last_tick: None,
        };
        particles.rebuild(device, queue, emitters, quality);
        particles
    }

    /// Starts every emitter of `emitters` afresh — pre-warmed, as at load —
    /// keeping the pipeline and every texture this pass ever tried: only a
    /// texture no emitter named before is downloaded, and one that was tried
    /// and failed keeps dropping its emitter rather than being fetched again
    /// (see [`Particles::slots`]). The simulation clock starts over with the
    /// emitters, so the first frame after a rebuild steps by nothing, the
    /// same as the first frame after a load.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        emitters: &[Emitter],
        quality: &QualityProfile,
    ) {
        self.live.clear();
        self.last_tick = None;
        if !self.enabled || emitters.is_empty() {
            return;
        }

        let references = untried(&self.slots, texture_refs(emitters));
        if !references.is_empty() {
            // Live-effect asset warnings aren't wired to the Output dock yet —
            // see `assets::load`'s doc comment; only scene-load-time warnings
            // are.
            let (images, _warnings) = assets::load(&references);
            let sampler =
                texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
            for reference in references {
                let slot = images.get(&reference).map(|image| {
                    let uploaded = texture::Uploaded::color(device, queue, image);
                    let bind_group = uploaded.bind(
                        device,
                        &self.image_layout,
                        &sampler,
                        quality.texture_max_size,
                    );
                    self.textures.push(Slot {
                        bind_group,
                        uploaded,
                    });
                    self.textures.len() - 1
                });
                self.slots.insert(reference, slot);
            }
        }

        self.live = emitters
            .iter()
            .filter_map(|emitter| {
                let texture = self.slots.get(&emitter.texture).copied().flatten()?;
                let mut simulation = Simulation::new(emitter.seed);
                simulation.prewarm(emitter);
                Some(Live {
                    emitter: emitter.clone(),
                    simulation,
                    texture,
                })
            })
            .collect();
    }

    /// Rebuilds the pipeline for a new sample count — the one thing about the
    /// target a quality change can alter.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        self.pipeline =
            pipeline::create_pipeline(device, target, &self.camera_layout, &self.image_layout);
    }

    /// Advances every emitter by wall-clock `dt`, sorts every alive particle
    /// back-to-front, and draws them in one pass over `targets`'s existing
    /// colour and depth attachments (loaded, not cleared, so this always runs
    /// after the opaque scene pass).
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
        for live in &mut self.live {
            live.simulation.step(&live.emitter, dt);
        }

        let items = self.collect(eye);
        self.upload(device, queue, &items);
        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraRaw {
                view_projection: view_projection.to_cols_array_2d(),
                eye: eye.to_array(),
                _pad: 0.0,
            }),
        );

        if items.is_empty() {
            return;
        }
        let Some(instances) = &self.instances else {
            return;
        };

        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview particles"),
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
        pass.set_vertex_buffer(0, self.quad.slice(..));
        pass.set_vertex_buffer(1, instances.slice(..));

        let mut start = 0usize;
        let mut current = items[0].texture;
        for (index, item) in items.iter().enumerate() {
            if item.texture != current {
                self.draw_run(&mut pass, current, start..index);
                start = index;
                current = item.texture;
            }
        }
        self.draw_run(&mut pass, current, start..items.len());
    }

    fn draw_run(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        texture: usize,
        range: std::ops::Range<usize>,
    ) {
        let Some(slot) = self.textures.get(texture) else {
            return;
        };
        pass.set_bind_group(1, &slot.bind_group, &[]);
        pass.draw(0..4, range.start as u32..range.end as u32);
    }

    /// Snapshots every alive particle across every emitter, in back-to-front
    /// order: farthest from the eye first, so nearer particles blend on top.
    fn collect(&self, eye: Vec3) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .live
            .iter()
            .flat_map(|live| {
                let texture = live.texture;
                let z_offset = live.emitter.z_offset;
                live.simulation
                    .particles(&live.emitter)
                    .filter_map(move |particle| {
                        if particle.alpha <= 0.0 || particle.size <= 0.0 {
                            return None;
                        }
                        let position = if z_offset != 0.0 {
                            particle.position
                                + (eye - particle.position).normalize_or_zero() * z_offset
                        } else {
                            particle.position
                        };
                        Some(Item {
                            texture,
                            distance: (position - eye).length_squared(),
                            raw: ParticleRaw {
                                position: position.to_array(),
                                size: particle.size,
                                color: particle.color,
                                alpha: particle.alpha,
                                rotation: particle.rotation,
                                light_emission: live.emitter.light_emission,
                            },
                        })
                    })
            })
            .collect();
        items.sort_by(|a, b| b.distance.total_cmp(&a.distance));
        items
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, items: &[Item]) {
        if items.is_empty() {
            return;
        }
        if items.len() > self.instance_capacity {
            self.instance_capacity = items.len();
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview particle instances"),
                size: (self.instance_capacity * std::mem::size_of::<ParticleRaw>())
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(buffer) = &self.instances else {
            return;
        };
        let raw: Vec<ParticleRaw> = items.iter().map(|item| item.raw).collect();
        queue.write_buffer(buffer, 0, bytemuck::cast_slice(&raw));
    }
}

/// Every distinct texture the given emitters need, in first-seen order — kept
/// stable so re-running the same file draws the same texture-to-slot mapping.
fn texture_refs(emitters: &[Emitter]) -> Vec<AssetRef> {
    let mut seen = Vec::new();
    for emitter in emitters {
        if !seen.contains(&emitter.texture) {
            seen.push(emitter.texture.clone());
        }
    }
    seen
}

#[cfg(test)]
#[path = "particles/tests.rs"]
mod tests;
