//! The 3D adornment family, drawn as unlit geometry over the scene:
//! `SelectionBox`'s bars, `SelectionSphere`'s ring, the `*HandleAdornment`
//! shapes, `Handles`' arrows and `ArcHandles`' rings.
//!
//! `scene::adornment` has already resolved every one of them into world
//! -space primitives, so this pass only turns those into triangles (see
//! [`geometry`]) and picks which of two depth behaviours to draw them with:
//! an adornment is documented as being "occluded by scene geometry like an
//! ordinary world object" by default, and as drawing over everything while
//! `AlwaysOnTop` — with `ZIndex` ordering the ones that do among
//! themselves, which is why the buffer is built in that order and drawn
//! straight through.
//!
//! Nothing here is back-face culled. An adornment is a thin overlay that a
//! camera is free to stand inside — a `SelectionBox` around the part the
//! eye is in, a `Handles` arrow the camera has flown past — and culling
//! would turn those into holes.
//!
//! Costs nothing in a place with no adornments: the pipelines are built the
//! first time one appears and never before, the same way
//! `renderer::highlight` builds its own.

mod geometry;
mod pipelines;

pub(super) use geometry::line;
pub(super) use pipelines::lines as line_pipelines;

use std::collections::HashMap;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::pipeline::Target;
use super::texture;
use crate::load::Answered;
use crate::quality::QualityProfile;
use crate::scene::{AdornPiece, Adornment};

use pipelines::Gpu;

/// One corner of a solid adornment triangle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

/// One corner of a line's screen-space quad: like `renderer::outline`'s, but
/// carrying its own width and colour, since every adornment picks both.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct LineVertex {
    position: [f32; 3],
    other: [f32; 3],
    side: f32,
    half_width: f32,
    color: [f32; 4],
}

/// One corner of an `ImageHandleAdornment`'s quad.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct ImageVertex {
    position: [f32; 3],
    uv: [f32; 2],
    alpha: f32,
}

/// A vertex buffer and where its depth-tested and always-on-top halves are.
#[derive(Default)]
pub(super) struct Batch {
    buffer: Option<wgpu::Buffer>,
    occluded: Range<u32>,
    on_top: Range<u32>,
}

impl Batch {
    pub(super) fn build<T: Pod>(
        device: &wgpu::Device,
        label: &str,
        occluded: &[T],
        on_top: &[T],
    ) -> Self {
        if occluded.is_empty() && on_top.is_empty() {
            return Batch::default();
        }
        let mut all = occluded.to_vec();
        all.extend_from_slice(on_top);
        Batch {
            buffer: Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(&all),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            ),
            occluded: 0..occluded.len() as u32,
            on_top: occluded.len() as u32..all.len() as u32,
        }
    }

    pub(super) fn range(&self, on_top: bool) -> Option<(&wgpu::Buffer, Range<u32>)> {
        let buffer = self.buffer.as_ref()?;
        let range = if on_top {
            self.on_top.clone()
        } else {
            self.occluded.clone()
        };
        (!range.is_empty()).then_some((buffer, range))
    }
}

/// One `ImageHandleAdornment`'s draw: which texture, which vertices.
struct Picture {
    bind_group: wgpu::BindGroup,
    range: Range<u32>,
    on_top: bool,
}

/// A `SelectionSphere`'s outline, kept as data rather than geometry: it
/// faces the eye, so its vertices are rebuilt whenever the camera moves.
struct RingSource {
    centre: Vec3,
    radius: f32,
    pixels: f32,
    color: [f32; 4],
    on_top: bool,
}

pub(super) struct Adornments {
    format: Target,
    gpu: Option<Gpu>,
    solids: Batch,
    lines: Batch,
    rings: Vec<RingSource>,
    /// The rings' vertices as of [`Adornments::prepare`]'s last eye, grown
    /// but never shrunk.
    ring_batch: Batch,
    ring_eye: Option<Vec3>,
    pictures: Vec<Picture>,
    picture_vertices: Option<wgpu::Buffer>,
    /// Every image an `ImageHandleAdornment` asked for, uploaded once and
    /// kept across re-plans — `None` for one the loader answered with a
    /// failure, which is not asked for again.
    uploaded: HashMap<AssetRef, Option<texture::Uploaded>>,
    sampler: wgpu::Sampler,
    image_layout: wgpu::BindGroupLayout,
    anisotropy: u16,
}

impl Adornments {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        adornments: &[Adornment],
        images: &Answered,
        quality: &QualityProfile,
    ) -> Self {
        let mut pass = Adornments {
            format: target,
            gpu: None,
            solids: Batch::default(),
            lines: Batch::default(),
            rings: Vec::new(),
            ring_batch: Batch::default(),
            ring_eye: None,
            pictures: Vec::new(),
            picture_vertices: None,
            uploaded: HashMap::new(),
            sampler: texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy),
            image_layout: texture::layout(device),
            anisotropy: quality.anisotropy,
        };
        pass.replace(device, queue, target, adornments, images);
        pass
    }

    /// Takes on a freshly planned adornment list, rebuilding every buffer:
    /// the geometry is baked world-space, so an adornment that moved is new
    /// triangles rather than a new transform.
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        adornments: &[Adornment],
        images: &Answered,
    ) {
        self.format = target;
        self.rings.clear();
        self.pictures.clear();
        self.picture_vertices = None;
        self.ring_batch = Batch::default();
        self.ring_eye = None;
        if adornments.is_empty() {
            self.solids = Batch::default();
            self.lines = Batch::default();
            return;
        }
        if self.gpu.is_none() {
            self.gpu = Some(Gpu::new(device, target, &self.image_layout));
        }

        // `ZIndex` orders the always-on-top ones among themselves, and the
        // pass draws straight through the buffer, so the order is baked in
        // here. The depth-tested ones keep DOM order, which is all a depth
        // test leaves to decide.
        let mut ordered: Vec<&Adornment> = adornments.iter().collect();
        ordered.sort_by_key(|adornment| adornment.order);

        let mut solids: [Vec<Vertex>; 2] = [Vec::new(), Vec::new()];
        let mut lines: [Vec<LineVertex>; 2] = [Vec::new(), Vec::new()];
        let mut pictures: Vec<ImageVertex> = Vec::new();
        let mut picture_draws: Vec<(AssetRef, Range<u32>, bool)> = Vec::new();

        for adornment in ordered {
            let side = usize::from(adornment.always_on_top);
            for piece in &adornment.pieces {
                match piece {
                    AdornPiece::Solid(solid) => geometry::solid(
                        solid.mesh,
                        solid.frame,
                        rgba(solid.color, solid.alpha),
                        &mut solids[side],
                    ),
                    AdornPiece::Line(line) => geometry::line(
                        line.from,
                        line.to,
                        line.pixels,
                        rgba(line.color, line.alpha),
                        &mut lines[side],
                    ),
                    AdornPiece::Ring(ring) => self.rings.push(RingSource {
                        centre: ring.centre,
                        radius: ring.radius,
                        pixels: ring.pixels,
                        color: rgba(ring.color, ring.alpha),
                        on_top: adornment.always_on_top,
                    }),
                    AdornPiece::Picture(picture) => {
                        let start = pictures.len() as u32;
                        geometry::picture_quad(picture, picture.alpha, &mut pictures);
                        picture_draws.push((
                            picture.texture.clone(),
                            start..pictures.len() as u32,
                            adornment.always_on_top,
                        ));
                    }
                }
            }
        }

        self.solids = Batch::build(device, "rbxview adornment solids", &solids[0], &solids[1]);
        self.lines = Batch::build(device, "rbxview adornment lines", &lines[0], &lines[1]);
        self.build_pictures(device, queue, images, &pictures, picture_draws);
    }

    fn build_pictures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &Answered,
        vertices: &[ImageVertex],
        draws: Vec<(AssetRef, Range<u32>, bool)>,
    ) {
        if vertices.is_empty() {
            return;
        }
        self.picture_vertices = Some(device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("rbxview adornment images"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX,
            },
        ));
        for (reference, range, on_top) in draws {
            if !self.uploaded.contains_key(&reference) {
                // No answer yet leaves the reference unrecorded, so the
                // rebuild that follows its landing picks it up — the same
                // rule `renderer::particles` follows for an emitter texture.
                let Some(answer) = images.get(&reference) else {
                    continue;
                };
                let uploaded = answer
                    .as_ref()
                    .map(|image| texture::Uploaded::color(device, queue, image));
                self.uploaded.insert(reference.clone(), uploaded);
            }
            let Some(Some(uploaded)) = self.uploaded.get(&reference) else {
                continue;
            };
            self.pictures.push(Picture {
                bind_group: uploaded.bind(
                    device,
                    &self.image_layout,
                    &self.sampler,
                    u32::from(self.anisotropy),
                ),
                range,
                on_top,
            });
        }
    }

    /// Follows a new sample count — see
    /// `renderer::switch::Renderer::rebuild_pipelines`.
    pub(super) fn set_target(&mut self, device: &wgpu::Device, target: Target) {
        self.format = target;
        if self.gpu.is_some() {
            self.gpu = Some(Gpu::new(device, target, &self.image_layout));
        }
    }

    /// Rebuilds the camera-facing rings, if this place has any and the eye
    /// has moved since they were last built. Outside the render pass,
    /// because it writes a buffer that pass reads.
    pub(super) fn prepare(&mut self, device: &wgpu::Device, eye: Vec3) {
        if self.rings.is_empty() || self.ring_eye == Some(eye) {
            return;
        }
        let mut sides: [Vec<LineVertex>; 2] = [Vec::new(), Vec::new()];
        for ring in &self.rings {
            geometry::ring(
                ring.centre,
                ring.radius,
                ring.pixels,
                ring.color,
                eye,
                &mut sides[usize::from(ring.on_top)],
            );
        }
        let [occluded, on_top] = sides;
        self.ring_batch = Batch::build(device, "rbxview adornment rings", &occluded, &on_top);
        self.ring_eye = Some(eye);
    }

    /// Draws every adornment into the scene pass: the depth-tested ones
    /// first, then the always-on-top ones over them.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        for on_top in [false, true] {
            if let Some((buffer, range)) = self.solids.range(on_top) {
                pass.set_pipeline(gpu.solid(on_top));
                pass.set_bind_group(0, frame, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range, 0..1);
            }
            for batch in [&self.lines, &self.ring_batch] {
                if let Some((buffer, range)) = batch.range(on_top) {
                    pass.set_pipeline(gpu.line(on_top));
                    pass.set_bind_group(0, frame, &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(range, 0..1);
                }
            }
            let Some(vertices) = &self.picture_vertices else {
                continue;
            };
            for picture in self.pictures.iter().filter(|one| one.on_top == on_top) {
                pass.set_pipeline(gpu.image(on_top));
                pass.set_bind_group(0, frame, &[]);
                pass.set_bind_group(1, &picture.bind_group, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.draw(picture.range.clone(), 0..1);
            }
        }
    }
}

/// A linear colour and its alpha, as the shaders take it.
fn rgba(color: [f32; 3], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)]
}
