//! The background pass: a `Sky`'s six panels, drawn before everything else.

use bytemuck::{Pod, Zeroable};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::envmap::sky_key;
use super::pipeline::{Frame, Shared, Target, DEPTH_FORMAT, SKYBOX_SHADER};
use super::texture;
use crate::quality::QualityProfile;
use crate::textures::{Panel, QUAD_INDICES};

const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
}

/// The six panels, their pipeline, and the translation-free camera they use.
pub(super) struct Skybox {
    pipeline: wgpu::RenderPipeline,
    pub(super) camera: Frame,
    /// The panels themselves, so a quality level can re-view them at another cap
    /// without decoding or uploading the sky again.
    uploads: Vec<texture::Uploaded>,
    image_layout: wgpu::BindGroupLayout,
    images: Vec<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Which sky the panels are (see `envmap::sky_key`), for a scene rebuild
    /// to keep them when it is still the same one.
    panels: Vec<AssetRef>,
}

/// Everything about a skybox that comes from its panels rather than from the
/// renderer: built once by [`Skybox::new`], and again by [`Skybox::replace`]
/// when a rebuild finds another sky.
struct Panels {
    uploads: Vec<texture::Uploaded>,
    images: Vec<wgpu::BindGroup>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
}

impl Skybox {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        shared: Shared<'_>,
        panels: &[Panel],
        quality: &QualityProfile,
    ) -> Self {
        let image_layout = texture::layout(device);
        let Panels {
            uploads,
            images,
            vertices,
            indices,
        } = upload(device, queue, &image_layout, panels, quality);

        Skybox {
            pipeline: create(device, target, &[Some(frame_layout), Some(&image_layout)]),
            camera: Frame::new(device, frame_layout, shared),
            uploads,
            image_layout,
            images,
            vertices,
            indices,
            panels: sky_key(Some(panels)).unwrap_or_default(),
        }
    }

    /// Whether the uploaded panels are exactly `panels` — see `envmap::sky_key`.
    pub(super) fn holds(&self, panels: &[Panel]) -> bool {
        sky_key(Some(panels)).is_some_and(|key| key == self.panels)
    }

    /// Swaps in another sky's panels, keeping the pipeline and the camera bind
    /// group: what a scene rebuild does for a `Sky` edit, rather than
    /// compiling the sky shader again.
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        panels: &[Panel],
        quality: &QualityProfile,
    ) {
        let Panels {
            uploads,
            images,
            vertices,
            indices,
        } = upload(device, queue, &self.image_layout, panels, quality);
        self.uploads = uploads;
        self.images = images;
        self.vertices = vertices;
        self.indices = indices;
        self.panels = sky_key(Some(panels)).unwrap_or_default();
    }

    /// Re-views the six panels at the new texture cap and anisotropy.
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

    /// Fills the frame before any geometry is drawn.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);

        let per_panel = QUAD_INDICES.len() as u32;
        for (panel, image) in self.images.iter().enumerate() {
            let first = u32::try_from(panel).unwrap_or(0) * per_panel;
            pass.set_bind_group(1, image, &[]);
            pass.draw_indexed(first..first + per_panel, 0, 0..1);
        }
    }
}

/// Uploads every panel and builds the quads they are pasted on.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image_layout: &wgpu::BindGroupLayout,
    panels: &[Panel],
    quality: &QualityProfile,
) -> Panels {
    // ClampToEdge: a panel is sampled right up to its border, and wrapping
    // there would pull the far edge of the image into the seam.
    let sampler = texture::sampler(device, wgpu::AddressMode::ClampToEdge, quality.anisotropy);

    let mut vertices = Vec::with_capacity(panels.len() * 4);
    let mut indices = Vec::with_capacity(panels.len() * QUAD_INDICES.len());
    let mut uploads = Vec::with_capacity(panels.len());
    let mut images = Vec::with_capacity(panels.len());
    for (offset, panel) in panels.iter().enumerate() {
        let base = (offset * 4) as u16;
        for corner in 0..4 {
            vertices.push(Vertex {
                position: panel.quad.positions[corner],
                uv: panel.quad.uvs[corner],
            });
        }
        indices.extend(QUAD_INDICES.iter().map(|index| base + index));
        let upload = texture::Uploaded::color(device, queue, &panel.image);
        images.push(upload.bind(device, image_layout, &sampler, quality.texture_max_size));
        uploads.push(upload);
    }

    Panels {
        uploads,
        images,
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview sky vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview sky indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
    }
}

fn create(
    device: &wgpu::Device,
    target: Target,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rbxview sky"),
        source: wgpu::ShaderSource::Wgsl(SKYBOX_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview sky"),
        bind_group_layouts,
        immediate_size: 0,
    });

    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview sky"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            // No culling: the panels carry the exterior-facing cube map basis
            // (see `textures::sky`), so their triangles wind away from the camera
            // sitting at the centre. Culling would blank the sky, and either
            // winding is fine for six flat quads that never overlap.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // The sky is pure background: it writes no depth and tests against
            // none, so the geometry drawn after it always wins.
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
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
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
