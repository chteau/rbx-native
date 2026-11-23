//! `ParticleEmitter2D` in the GUI pass. The rest of a GUI is static and
//! built once per layout; particles move every frame, so each layout keeps
//! a list of its emitters (see `quads::build`), [`Sims`] steps them once a
//! frame with wall-clock time, and a [`Layer`] draws each emitter's quads
//! from a buffer of its own at the place in the paint order its run marks:
//! right after its parent's own paint, under the parent's children, which is
//! where Studio draws them whatever the emitter's `ZIndex`.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use rbx_assets::AssetRef;
use rbx_dom::Ref;
use web_time::Instant;

use super::atlas::Slot;
use crate::scene::{FlatParticle, FlatSim, GuiParticles};

const SHADER: &str = include_str!("../gui_particles.wgsl");

/// The longest step one frame advances by: after a hitch, or a host that
/// stopped drawing for a while, the effect carries on rather than jumping a
/// whole lifetime ahead.
const MAX_STEP: f32 = 0.1;
/// How many frames a simulation no layout drew is kept: long enough to
/// survive a rebuild (an edit to the emitter re-plans the whole tree and
/// must not clear its particles, it doesn't in Studio), short enough that a
/// deleted emitter's state goes.
const FORGET_AFTER: u64 = 120;

static BURSTS: Mutex<Vec<(Ref, Option<u32>)>> = Mutex::new(Vec::new());

/// A script's `ParticleEmitter2D:Emit(count)` (`Some`) or `Clear()` (`None`),
/// applied on the next frame drawn.
pub fn gui_particle_burst(emitter: Ref, burst: Option<u32>) {
    if let Ok(mut queue) = BURSTS.lock() {
        queue.push((emitter, burst));
    }
}

/// Whether a burst is waiting for the next frame, which a host that only
/// draws on change has to draw for.
fn bursts_pending() -> bool {
    BURSTS.lock().is_ok_and(|queue| !queue.is_empty())
}

fn take_bursts() -> Vec<(Ref, Option<u32>)> {
    BURSTS
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

struct Live {
    sim: FlatSim,
    /// The frame this was last stepped in: an emitter on the overlay and on
    /// an editor's canvas at once still only moves once a frame.
    stepped: u64,
    seen: u64,
}

/// Every running `ParticleEmitter2D`, by referent.
pub(super) struct Sims {
    live: HashMap<Ref, Live>,
    last: Option<Instant>,
    frame: u64,
    dt: f32,
    /// Whether a build since the last tick, and since the one before, had
    /// anything that moves: the overlay and an editor's canvas each tick.
    moving: [bool; 2],
    /// See [`Sims::hold`].
    held: bool,
}

/// One particle corner.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct VertexRaw {
    pub(super) position: [f32; 2],
    pub(super) uv: [f32; 2],
    pub(super) uv_next: [f32; 2],
    pub(super) between: f32,
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
    pub(super) brightness: f32,
    pub(super) blend: f32,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32,
    4 => Float32x3,
    5 => Float32,
    6 => Float32,
    7 => Float32,
];

/// Each emitter's texture and slice of the vertices, `None` where it has
/// nothing to draw (no particles, or a texture not loaded).
pub(super) type Ranges = Vec<Option<(usize, Range<u32>)>>;

impl Sims {
    pub(super) fn new() -> Self {
        Sims {
            live: HashMap::new(),
            last: None,
            frame: 0,
            dt: 0.0,
            moving: [false; 2],
            held: false,
        }
    }

    /// Makes the next [`Sims::tick`] keep this frame: the same picture drawn
    /// again (over another backdrop) has to show the same particles.
    pub(super) fn hold(&mut self) {
        self.held = true;
    }

    /// Starts a frame: how far every emitter moves in it, and whatever
    /// `Emit`/`Clear` scripts asked for since the last one.
    pub(super) fn tick(&mut self) {
        if std::mem::take(&mut self.held) {
            return;
        }
        let now = Instant::now();
        self.dt = self
            .last
            .map_or(0.0, |last| (now - last).as_secs_f32().min(MAX_STEP));
        self.last = Some(now);
        self.frame += 1;
        self.moving = [false, self.moving[0]];
        for (referent, burst) in take_bursts() {
            let live = self.entry(referent);
            match burst {
                Some(count) => live.sim.emit(count),
                None => live.sim.clear(),
            }
        }
        let frame = self.frame;
        self.live
            .retain(|_, live| frame.saturating_sub(live.seen) < FORGET_AFTER);
    }

    fn entry(&mut self, referent: Ref) -> &mut Live {
        let frame = self.frame;
        self.live.entry(referent).or_insert_with(|| Live {
            sim: FlatSim::seeded(referent),
            stepped: 0,
            seen: frame,
        })
    }

    /// Whether the picture changes from frame to frame: some emitter is
    /// spawning or has particles in flight.
    pub(super) fn moving(&self) -> bool {
        self.moving[0] || self.moving[1] || bursts_pending()
    }

    /// Steps every emitter in `slots` (once a frame) and builds its quads.
    pub(super) fn build(
        &mut self,
        slots: &[GuiParticles],
        slot_of: &HashMap<AssetRef, Slot>,
    ) -> (Vec<VertexRaw>, Ranges) {
        let mut vertices = Vec::new();
        let mut ranges = Vec::with_capacity(slots.len());
        let (frame, dt) = (self.frame, self.dt);
        let mut moving = false;
        for slot in slots {
            let emitter = &slot.emitter;
            let live = self.entry(emitter.referent);
            live.seen = frame;
            if live.stepped != frame {
                live.stepped = frame;
                live.sim.step(emitter, slot.frame, dt);
            }
            let running = emitter.time_scale > 0.0;
            moving |= running && (!live.sim.is_empty() || (emitter.enabled && emitter.rate > 0.0));
            let texture = emitter
                .texture
                .as_ref()
                .and_then(|texture| slot_of.get(texture));
            let Some(texture) = texture else {
                ranges.push(None);
                continue;
            };
            let start = vertices.len() as u32;
            let book = emitter.flipbook;
            for particle in live.sim.particles(emitter, slot.frame) {
                quad(
                    &particle,
                    emitter.brightness,
                    emitter.blend,
                    (book.columns, book.rows),
                    &mut vertices,
                );
            }
            let end = vertices.len() as u32;
            ranges.push((end > start).then_some((texture.linear, start..end)));
        }
        self.moving[0] |= moving;
        (vertices, ranges)
    }
}

/// One particle's two triangles, turned about its centre.
fn quad(
    particle: &FlatParticle,
    brightness: f32,
    blend: f32,
    (columns, rows): (u32, u32),
    into: &mut Vec<VertexRaw>,
) {
    let (sin, cos) = particle.rotation.to_radians().sin_cos();
    let half = [particle.extent[0] * 0.5, particle.extent[1] * 0.5];
    let cell = |frame: u32| {
        let (column, row) = (frame % columns, frame / columns);
        let size = [1.0 / columns as f32, 1.0 / rows as f32];
        ([column as f32 * size[0], row as f32 * size[1]], size)
    };
    let (frame, next, between) = particle.frames;
    let (origin, size) = cell(frame);
    let (origin_next, _) = cell(next);
    let corner = |u: f32, v: f32| {
        let x = (u * 2.0 - 1.0) * half[0];
        let y = (v * 2.0 - 1.0) * half[1];
        VertexRaw {
            position: [
                particle.center[0] + x * cos - y * sin,
                particle.center[1] + x * sin + y * cos,
            ],
            uv: [origin[0] + u * size[0], origin[1] + v * size[1]],
            uv_next: [origin_next[0] + u * size[0], origin_next[1] + v * size[1]],
            between,
            color: particle.color,
            alpha: particle.alpha,
            brightness,
            blend,
        }
    };
    let [a, b, c, d] = [
        corner(0.0, 0.0),
        corner(1.0, 0.0),
        corner(1.0, 1.0),
        corner(0.0, 1.0),
    ];
    into.extend_from_slice(&[a, b, c, a, c, d]);
}

/// The GPU half: a pipeline and a vertex buffer refilled every frame.
pub(super) struct Layer {
    pipeline: wgpu::RenderPipeline,
    vertices: Option<wgpu::Buffer>,
    capacity: usize,
    ranges: Ranges,
}

impl Layer {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport_layout: &wgpu::BindGroupLayout,
        image_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        Layer {
            pipeline: pipeline(device, format, viewport_layout, image_layout),
            vertices: None,
            capacity: 0,
            ranges: Vec::new(),
        }
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (vertices, ranges): (Vec<VertexRaw>, Ranges),
    ) {
        self.ranges = ranges;
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.capacity {
            self.capacity = vertices.len().next_power_of_two();
            self.vertices = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview gui particles"),
                size: (self.capacity * std::mem::size_of::<VertexRaw>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = &self.vertices {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&vertices));
        }
    }

    /// Draws emitter `index`'s particles, leaving the pass on this layer's
    /// pipeline and buffer; `false` when there was nothing to draw and the
    /// pass is untouched.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        index: usize,
        viewport: &wgpu::BindGroup,
        textures: &[wgpu::BindGroup],
    ) -> bool {
        let Some(Some((texture, range))) = self.ranges.get(index) else {
            return false;
        };
        let (Some(buffer), Some(texture)) = (&self.vertices, textures.get(*texture)) else {
            return false;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, viewport, &[]);
        pass.set_bind_group(1, texture, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(range.clone(), 0..1);
        true
    }
}

fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    viewport_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = crate::gpu::shader(device, "rbxview gui particles", SHADER);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview gui particles"),
        bind_group_layouts: &[Some(viewport_layout), Some(image_layout)],
        immediate_size: 0,
    });
    let premultiplied = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview gui particles"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<VertexRaw>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: premultiplied,
                    alpha: premultiplied,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests;
