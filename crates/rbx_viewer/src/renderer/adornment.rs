//! The 3D adornment family, drawn as unlit geometry over the scene:
//! `SelectionBox`'s bars, `SelectionSphere`'s ring, the `*HandleAdornment`
//! shapes, `Handles`' arrows and `ArcHandles`' rings.
//!
//! `scene::adornment` has already resolved every one of them into world
//! -space primitives, so this pass only turns those into triangles (see
//! [`geometry`]) and draws them the way Studio does, measured 2026-11-24:
//!
//! - A depth-tested adornment is blended in sRGB values at its own opacity
//!   *before* Studio's tone curve, so it is drawn into a layer of its own
//!   (`post::Targets::adornment_layer`) that the resolve lays over the frame
//!   through that curve (`post.wgsl`'s `composite_adornments`). An opaque one
//!   also writes an adornment-only depth: depth-tested adornments hide each
//!   other by depth, `ZIndex` plays no part, and two crossing boxes cut
//!   each other along their intersection.
//! - An `AlwaysOnTop` one is blended in sRGB values onto the finished frame,
//!   after the curve (white stays 255), tested only against that adornment
//!   depth: parts never hide it, a nearer opaque depth-tested adornment
//!   does, a tie goes to it. Never against each other: they are painted in
//!   `ZIndex` order, then far to near, then creation order.
//! - The blended depth-tested ones write no depth and are painted far to
//!   near after the opaque ones.
//! - Every tie goes to the adornment created later (DOM order here).
//!
//! Only a `HandleAdornment` shape with `DoubleSided = false` (its default)
//! drops the faces turned away from the eye: stand inside a
//! `BoxHandleAdornment` in Studio and it vanishes, look through a
//! transparent one and its far side is gone. Each vertex carries that flag
//! and the fragment shader discards a back face that has it, so one
//! pipeline draws both kinds in whatever order the passes need. Everything
//! else (a
//! `SelectionBox` around the part the eye is in, a `Handles` arrow the
//! camera has flown past) has no such property and stays two-sided.
//!
//! Costs nothing in a place with no adornments: the pipelines are built the
//! first time one appears and never before, the same way
//! `renderer::highlight` builds its own.

mod geometry;
mod passes;
mod pipelines;

pub(super) use geometry::line;

use std::collections::HashMap;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::texture;
use crate::load::Answered;
use crate::quality::QualityProfile;
use crate::scene::{linear_to_srgb, AdornPiece, Adornment};

use passes::draw_order;
use pipelines::{Layer, Top};

/// One corner of a solid adornment triangle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    /// sRGB-encoded, unlike every other colour the renderer is handed:
    /// Studio blends adornments in sRGB values (see this module's docs).
    color: [f32; 4],
    /// 1 to drop this triangle's back face, `DoubleSided = false`.
    single_sided: f32,
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
    /// The tint, sRGB, and the opacity, as [`Vertex::color`].
    color: [f32; 4],
    /// As [`Vertex::single_sided`].
    single_sided: f32,
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

/// One draw of an adornment's: which buffer, which vertices.
#[derive(Debug, Clone, PartialEq)]
enum Draw {
    Solid(Range<u32>),
    Line(Range<u32>),
    /// Index into [`Adornments::rings`], whose vertices follow the eye.
    Ring(usize),
    /// Index into [`Adornments::images`].
    Picture(usize, Range<u32>),
}

/// One adornment, as the passes order it.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    draws: Vec<Draw>,
    on_top: bool,
    /// Fully opaque and textureless, the only kind that writes the
    /// adornment depth.
    opaque: bool,
    order: i32,
    centre: Vec3,
}

/// A `SelectionSphere`'s outline, kept as data rather than geometry: it
/// faces the eye, so its vertices are rebuilt whenever the camera moves.
struct RingSource {
    centre: Vec3,
    radius: f32,
    pixels: f32,
    color: [f32; 4],
    range: Range<u32>,
}

pub(super) struct Adornments {
    /// Built the first frame an adornment is drawn, for the scene's sample
    /// count (the layer) and the frame's format (on top).
    layer_gpu: Option<(u32, Layer)>,
    top_gpu: Option<(wgpu::TextureFormat, Top)>,
    solids: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    pictures: Option<wgpu::Buffer>,
    images: Vec<wgpu::BindGroup>,
    entries: Vec<Entry>,
    rings: Vec<RingSource>,
    ring_vertices: Option<wgpu::Buffer>,
    /// The camera [`Adornments::prepare`] last sorted and faced rings for.
    camera: Option<(Vec3, Mat4)>,
    /// This frame's draw order: the depth-tested entries, then the
    /// always-on-top ones.
    layered: Vec<usize>,
    on_top: Vec<usize>,
    /// Whether the layer still holds a frame's adornments, so a frame with
    /// none clears it once rather than every frame.
    layer_dirty: bool,
    /// Whether this frame's layer pass cleared the adornment depth, which
    /// the on-top pass then tests against instead of clearing it itself.
    depth_written: bool,
    /// Every image an `ImageHandleAdornment` asked for, uploaded once and
    /// kept across re-plans, `None` for one the loader answered with a
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
        adornments: &[Adornment],
        images: &Answered,
        quality: &QualityProfile,
    ) -> Self {
        let mut pass = Adornments {
            layer_gpu: None,
            top_gpu: None,
            solids: None,
            lines: None,
            pictures: None,
            images: Vec::new(),
            entries: Vec::new(),
            rings: Vec::new(),
            ring_vertices: None,
            camera: None,
            layered: Vec::new(),
            on_top: Vec::new(),
            layer_dirty: false,
            depth_written: false,
            uploaded: HashMap::new(),
            sampler: texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy),
            image_layout: texture::layout(device),
            anisotropy: quality.anisotropy,
        };
        pass.replace(device, queue, adornments, images);
        pass
    }

    /// Takes on a freshly planned adornment list, rebuilding every buffer:
    /// the geometry is baked world-space, so an adornment that moved is new
    /// triangles rather than a new transform.
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        adornments: &[Adornment],
        images: &Answered,
    ) {
        self.entries.clear();
        self.rings.clear();
        self.images.clear();
        self.ring_vertices = None;
        self.camera = None;
        self.layered.clear();
        self.on_top.clear();

        let mut solids: Vec<Vertex> = Vec::new();
        let mut lines: Vec<LineVertex> = Vec::new();
        let mut pictures: Vec<ImageVertex> = Vec::new();
        for adornment in adornments {
            let mut draws = Vec::new();
            let mut opaque = true;
            for piece in &adornment.pieces {
                match piece {
                    AdornPiece::Solid(solid) => {
                        let start = solids.len();
                        geometry::solid(
                            solid.mesh,
                            solid.frame,
                            encoded(solid.color, solid.alpha),
                            &mut solids,
                        );
                        if !solid.double_sided {
                            for vertex in &mut solids[start..] {
                                vertex.single_sided = 1.0;
                            }
                        }
                        opaque &= solid.alpha >= 1.0;
                        draws.push(Draw::Solid(start as u32..solids.len() as u32));
                    }
                    AdornPiece::Line(line) => {
                        let start = lines.len() as u32;
                        geometry::line(
                            line.from,
                            line.to,
                            line.pixels,
                            line_color(line.color, line.alpha, adornment.always_on_top),
                            &mut lines,
                        );
                        opaque &= line.alpha >= 1.0;
                        draws.push(Draw::Line(start..lines.len() as u32));
                    }
                    AdornPiece::Ring(ring) => {
                        opaque &= ring.alpha >= 1.0;
                        draws.push(Draw::Ring(self.rings.len()));
                        self.rings.push(RingSource {
                            centre: ring.centre,
                            radius: ring.radius,
                            pixels: ring.pixels,
                            color: encoded(ring.color, ring.alpha),
                            range: 0..0,
                        });
                    }
                    AdornPiece::Picture(picture) => {
                        // An image can carry its own alpha, so it is never
                        // treated as opaque.
                        opaque = false;
                        let start = pictures.len() as u32;
                        geometry::picture_quad(
                            picture,
                            encoded(picture.color, picture.alpha),
                            &mut pictures,
                        );
                        if let Some(image) = self.image(device, queue, images, &picture.texture) {
                            draws.push(Draw::Picture(image, start..pictures.len() as u32));
                        }
                    }
                }
            }
            self.entries.push(Entry {
                draws,
                on_top: adornment.always_on_top,
                opaque,
                order: adornment.order,
                centre: adornment.centre(),
            });
        }
        self.solids = vertex_buffer(device, "rbxview adornment solids", &solids);
        self.lines = vertex_buffer(device, "rbxview adornment lines", &lines);
        self.pictures = vertex_buffer(device, "rbxview adornment images", &pictures);
    }

    /// The bind group of the image `reference` names, uploading it the first
    /// time, or `None` while it has no answer or failed.
    fn image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: &Answered,
        reference: &AssetRef,
    ) -> Option<usize> {
        if !self.uploaded.contains_key(reference) {
            // No answer yet leaves the reference unrecorded, so the
            // rebuild that follows its landing picks it up, the same
            // rule `renderer::particles` follows for an emitter texture.
            let answer = images.get(reference)?;
            let uploaded = answer
                .as_ref()
                .map(|image| texture::Uploaded::color(device, queue, image));
            self.uploaded.insert(reference.clone(), uploaded);
        }
        let uploaded = self.uploaded.get(reference)?.as_ref()?;
        self.images.push(uploaded.bind(
            device,
            &self.image_layout,
            &self.sampler,
            u32::from(self.anisotropy),
        ));
        Some(self.images.len() - 1)
    }

    /// Faces the rings at `eye` and sorts this frame's draws for the camera
    /// `view_projection` describes, if it has moved since. Outside the
    /// render passes, because it writes a buffer they read.
    pub(super) fn prepare(&mut self, device: &wgpu::Device, eye: Vec3, view_projection: Mat4) {
        if self.entries.is_empty() || self.camera == Some((eye, view_projection)) {
            return;
        }
        self.camera = Some((eye, view_projection));
        if !self.rings.is_empty() {
            let mut vertices: Vec<LineVertex> = Vec::new();
            for ring in &mut self.rings {
                let start = vertices.len() as u32;
                geometry::ring(
                    ring.centre,
                    ring.radius,
                    ring.pixels,
                    ring.color,
                    eye,
                    &mut vertices,
                );
                ring.range = start..vertices.len() as u32;
            }
            self.ring_vertices = vertex_buffer(device, "rbxview adornment rings", &vertices);
        }
        (self.layered, self.on_top) = draw_order(&self.entries, view_projection);
    }
}

fn vertex_buffer<T: Pod>(
    device: &wgpu::Device,
    label: &str,
    vertices: &[T],
) -> Option<wgpu::Buffer> {
    (!vertices.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        })
    })
}

/// A linear colour as the sRGB values Studio blends adornments in, and its
/// opacity.
fn encoded(color: [f32; 3], alpha: f32) -> [f32; 4] {
    let [r, g, b] = color.map(linear_to_srgb);
    [r, g, b, alpha.clamp(0.0, 1.0)]
}

/// A `LineHandleAdornment`'s colour: an `AlwaysOnTop` one draws at half
/// its `Color3` (measured: white lands on 127 over every background), a
/// depth-tested one at its full colour.
fn line_color(color: [f32; 3], alpha: f32, on_top: bool) -> [f32; 4] {
    let [r, g, b, a] = encoded(color, alpha);
    let scale = if on_top { 0.5 } else { 1.0 };
    [r * scale, g * scale, b * scale, a]
}

#[cfg(test)]
#[path = "adornment/tests.rs"]
mod tests;
