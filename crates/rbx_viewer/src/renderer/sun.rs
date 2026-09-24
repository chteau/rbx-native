//! The celestial bodies: `Sky`'s sun and moon, drawn as additive discs on the
//! sky right after it.
//!
//! Each is one quad the vertex shader turns to face the camera, so the only
//! per-body data is its angular size and which end of the light direction it
//! sits on.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::pipeline::{Frame, Shared, Target, DEPTH_FORMAT};
use super::texture;
use crate::quality::QualityProfile;
use crate::textures::{Body, Celestial, QUAD_INDICES};

/// How far outside the visible frame the sun's projection may still fall
/// before `SunRaysEffect` gives up on the frame rather than blur toward a
/// point nowhere near the image — a whole extra frame-width of slack on every
/// side, well past where the effect's own taps could ever reach.
const OFF_SCREEN_MARGIN: f32 = 1.0;

/// Projects a world direction through a translation-free view-projection
/// matrix, exactly like `vs_main` in sun.wgsl projects the sun disc itself —
/// reused here (rather than re-derived) so `renderer::post`'s god-rays can
/// never point anywhere but where the disc is actually drawn.
///
/// `None` where the direction is behind the camera: under this reversed-Z
/// perspective matrix `clip.w` is the (negated) view-space depth, positive in
/// front of the eye and zero or negative behind it, where the divide below
/// would be meaningless.
pub(super) fn project_direction(view_rotation_projection: &Mat4, direction: Vec3) -> Option<Vec2> {
    let unit = direction.try_normalize()?;
    let clip = *view_rotation_projection * unit.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }

    let ndc = clip.truncate() / clip.w;
    // Clip space runs [-1, 1] with Y up; a texture's UV runs [0, 1] with V
    // down, hence the flip on the second component only.
    Some(Vec2::new(ndc.x * 0.5 + 0.5, ndc.y * -0.5 + 0.5))
}

/// The sun's screen-space position for this frame's god-rays, or `None` where
/// `SunRaysEffect` must not draw at all this frame.
///
/// Three independent reasons collapse to the same `None`: the moon is lit
/// instead of the sun (Roblox's own effect never rays from the moon — no
/// fixture contradicts this, so it is this renderer's own reasonable
/// assumption, made the same way `renderer.rs`'s `sun_shadow` already picks
/// which lamp is "above the horizon" from the sign of `sun_direction.y`), the
/// direction is behind the camera, or the projection lands far enough outside
/// the frame that a radial blur toward it would just be noise.
pub(super) fn sun_screen_position(
    sun_direction: Vec3,
    view_rotation_projection: &Mat4,
) -> Option<Vec2> {
    if sun_direction.y < 0.0 {
        return None;
    }

    let uv = project_direction(view_rotation_projection, sun_direction)?;
    let visible = -OFF_SCREEN_MARGIN..=1.0 + OFF_SCREEN_MARGIN;
    (visible.contains(&uv.x) && visible.contains(&uv.y)).then_some(uv)
}

const SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("sun.wgsl")
);

const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x2];

/// Additive: a disc brightens the sky it sits on instead of replacing it, which
/// is what makes the sun read as a light source rather than a sticker.
const ADDITIVE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::REPLACE,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    corner: [f32; 2],
    uv: [f32; 2],
    extent: [f32; 2],
}

/// The bodies, their pipeline, and the translation-free camera they share with
/// the skybox (kept separately so a place with celestial bodies but no usable
/// panels still gets them).
pub(super) struct Bodies {
    pipeline: wgpu::RenderPipeline,
    pub(super) camera: Frame,
    /// The two discs' images, so a quality level can re-view them at another cap
    /// without uploading them again.
    uploads: Vec<texture::Uploaded>,
    image_layout: wgpu::BindGroupLayout,
    images: Vec<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// What the buffers were built from (see [`bodies_key`]), for a scene
    /// rebuild to keep them when the `Sky` still asks for the same discs.
    bodies: Vec<(AssetRef, Body)>,
}

/// The identity of the discs as far as their uploads go: each one's image
/// asset and the angular size and side its quad is built for. Two skies with
/// the same key upload the same images and the same quads.
fn bodies_key(bodies: &[Celestial]) -> Vec<(AssetRef, Body)> {
    bodies
        .iter()
        .map(|body| (body.reference.clone(), body.body))
        .collect()
}

/// Everything about the pass that comes from the discs rather than from the
/// renderer: built once by [`Bodies::new`], and again by [`Bodies::replace`]
/// when a rebuild finds other discs.
struct Discs {
    uploads: Vec<texture::Uploaded>,
    images: Vec<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
}

impl Bodies {
    /// `None` when the scene asks for no bodies at all, so the renderer can skip
    /// the pass rather than draw nothing.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        shared: Shared<'_>,
        bodies: &[Celestial],
        quality: &QualityProfile,
    ) -> Option<Self> {
        if bodies.is_empty() {
            return None;
        }

        let image_layout = texture::layout(device);
        let Discs {
            uploads,
            images,
            vertices,
            indices,
        } = upload(device, queue, &image_layout, bodies, quality);

        Some(Bodies {
            pipeline: create(device, target, &[Some(frame_layout), Some(&image_layout)]),
            camera: Frame::new(device, frame_layout, shared),
            uploads,
            image_layout,
            images,
            vertices,
            indices,
            bodies: bodies_key(bodies),
        })
    }

    /// Whether the uploaded discs are exactly `bodies` — see [`bodies_key`].
    pub(super) fn holds(&self, bodies: &[Celestial]) -> bool {
        self.bodies == bodies_key(bodies)
    }

    /// Swaps in other discs, keeping the pipeline and the camera bind group:
    /// what a scene rebuild does for a `Sky` edit that changed a texture or an
    /// angular size. `bodies` must not be empty — a place with none drops the
    /// pass instead (see [`Bodies::new`]).
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bodies: &[Celestial],
        quality: &QualityProfile,
    ) {
        let Discs {
            uploads,
            images,
            vertices,
            indices,
        } = upload(device, queue, &self.image_layout, bodies, quality);
        self.uploads = uploads;
        self.images = images;
        self.vertices = vertices;
        self.indices = indices;
        self.bodies = bodies_key(bodies);
    }

    /// Re-views the discs at the new texture cap and anisotropy.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);
        self.images = self
            .uploads
            .iter()
            .map(|upload| {
                upload.bind(
                    device,
                    &self.image_layout,
                    &sampler,
                    quality.texture_max_size,
                )
            })
            .collect();
    }

    /// Rebuilds the pipeline for a new sample count.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) {
        self.pipeline = create(
            device,
            target,
            &[Some(frame_layout), Some(&self.image_layout)],
        );
    }

    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);

        let per_body = QUAD_INDICES.len() as u32;
        for (body, image) in self.images.iter().enumerate() {
            let first = u32::try_from(body).unwrap_or(0) * per_body;
            pass.set_bind_group(1, image, &[]);
            pass.draw_indexed(first..first + per_body, 0, 0..1);
        }
    }
}

/// Uploads every disc and builds the quads they are drawn on.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image_layout: &wgpu::BindGroupLayout,
    bodies: &[Celestial],
    quality: &QualityProfile,
) -> Discs {
    // ClampToEdge: a disc is a single image, and wrapping at its border would
    // smear the sky's own colour round the outside of the sun.
    let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);

    let mut vertices = Vec::with_capacity(bodies.len() * 4);
    let mut indices = Vec::with_capacity(bodies.len() * QUAD_INDICES.len());
    let mut uploads = Vec::with_capacity(bodies.len());
    let mut images = Vec::with_capacity(bodies.len());
    for (offset, body) in bodies.iter().enumerate() {
        let base = u16::try_from(offset * 4).unwrap_or(0);
        vertices.extend(quad(body));
        indices.extend(QUAD_INDICES.iter().map(|index| base + index));
        let upload = texture::Uploaded::color(device, queue, &body.image);
        images.push(upload.bind(device, image_layout, &sampler, quality.texture_max_size));
        uploads.push(upload);
    }

    Discs {
        uploads,
        images,
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview celestial vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview celestial indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
    }
}

/// The four corners of one body's quad, in the image order [`QUAD_INDICES`]
/// winds: top-left, top-right, bottom-right, bottom-left.
///
/// `angular_size` is the disc's angular *diameter* in degrees, so the quad
/// reaches `tan` of half of it either side of the direction — which is exactly
/// right on a unit direction vector, whatever the far plane is.
fn quad(body: &Celestial) -> [Vertex; 4] {
    let extent = [
        (body.body.angular_size.to_radians() / 2.0).tan(),
        if body.body.toward_sun { 1.0 } else { -1.0 },
    ];

    let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    std::array::from_fn(|index| Vertex {
        corner: [corners[index].0, corners[index].1],
        uv: uvs[index],
        extent,
    })
}

fn create(
    device: &wgpu::Device,
    target: Target,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> wgpu::RenderPipeline {
    let shader = crate::gpu::shader(device, "rbxview celestial", SHADER);

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview celestial"),
        bind_group_layouts,
        immediate_size: 0,
    });

    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview celestial"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            // The quad is built around a direction that can point anywhere, so
            // its winding depends on the time of day; culling would blink the
            // sun out for half the sky.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            // GreaterEqual against a depth buffer still holding its clear value:
            // the disc passes over the bare sky and fails wherever geometry has
            // already written a nearer (bigger, under reversed-Z) depth.
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target.format,
                blend: Some(ADDITIVE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
#[path = "sun/tests.rs"]
mod tests;
