//! `Sky.StarCount` on the GPU: one camera-facing quad per star, drawn
//! additively over the sky and faded out by daylight.
//!
//! The directions come from [`crate::textures::stars`], which is where the
//! field is generated and unit-tested; this module only turns them into
//! vertices.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::pipeline::{Frame, Shared, Target, DEPTH_FORMAT};
use crate::textures::{Star, QUAD_INDICES};

const SHADER: &str = concat!(
    include_str!("lights.wgsl"),
    include_str!("lighting.wgsl"),
    include_str!("atmosphere.wgsl"),
    include_str!("stars.wgsl")
);

const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];

/// Additive, like the celestial bodies: a star brightens the sky it sits on.
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
    direction: [f32; 3],
    corner: [f32; 2],
    magnitude: f32,
}

/// The whole field in one draw: it never changes, so it is one vertex buffer
/// built once and a single indexed draw per frame.
pub(super) struct Stars {
    pipeline: wgpu::RenderPipeline,
    pub(super) camera: Frame,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    indices_count: u32,
    /// How many stars the buffers hold. `textures::stars::field` generates
    /// the same directions for the same count every time, so this alone tells
    /// a scene rebuild whether the buffers are still right (see
    /// [`Stars::holds`]).
    count: usize,
}

impl Stars {
    /// `None` where the place asks for no stars at all, so the renderer can skip
    /// the pass rather than draw an empty buffer.
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        shared: Shared<'_>,
        field: &[Star],
    ) -> Option<Self> {
        if field.is_empty() {
            return None;
        }

        let (vertices, indices) = buffers(device, field);
        Some(Stars {
            pipeline: create(device, target, &[Some(frame_layout)]),
            camera: Frame::new(device, frame_layout, shared),
            indices_count: index_count(field),
            vertices,
            indices,
            count: field.len(),
        })
    }

    /// Whether the buffers already hold `field` — see [`Stars::count`].
    pub(super) fn holds(&self, field: &[Star]) -> bool {
        self.count == field.len()
    }

    /// Swaps in another field, keeping the pipeline and the camera bind group:
    /// what a scene rebuild does for a `StarCount` edit. `field` must not be
    /// empty — a place with no stars drops the pass instead (see
    /// [`Stars::new`]).
    pub(super) fn replace(&mut self, device: &wgpu::Device, field: &[Star]) {
        let (vertices, indices) = buffers(device, field);
        self.vertices = vertices;
        self.indices = indices;
        self.indices_count = index_count(field);
        self.count = field.len();
    }

    /// Rebuilds the pipeline for a new sample count.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) {
        self.pipeline = create(device, target, &[Some(frame_layout)]);
    }

    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.indices_count, 0, 0..1);
    }
}

fn index_count(field: &[Star]) -> u32 {
    u32::try_from(field.len() * QUAD_INDICES.len()).unwrap_or(0)
}

/// One quad per star, as a vertex buffer and the index buffer that winds it.
fn buffers(device: &wgpu::Device, field: &[Star]) -> (wgpu::Buffer, wgpu::Buffer) {
    let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
    let mut vertices = Vec::with_capacity(field.len() * corners.len());
    let mut indices = Vec::with_capacity(field.len() * QUAD_INDICES.len());
    for (offset, star) in field.iter().enumerate() {
        // u32 indices, not the u16 every other quad pass uses: a default
        // field is 3000 stars, which is twelve thousand vertices.
        let base = u32::try_from(offset * corners.len()).unwrap_or(0);
        vertices.extend(corners.map(|corner| Vertex {
            direction: star.direction.to_array(),
            corner: [corner.0, corner.1],
            magnitude: star.magnitude,
        }));
        indices.extend(QUAD_INDICES.iter().map(|&index| base + u32::from(index)));
    }

    (
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview star vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview star indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
    )
}

fn create(
    device: &wgpu::Device,
    target: Target,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> wgpu::RenderPipeline {
    let shader = crate::gpu::shader(device, "rbxview stars", SHADER);

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rbxview stars"),
        bind_group_layouts,
        immediate_size: 0,
    });

    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    })];

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rbxview stars"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            // A quad built around an arbitrary direction winds either way, as
            // the celestial bodies' do.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            // GreaterEqual against the cleared depth buffer: a star passes over
            // the bare sky and fails wherever geometry already stands.
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
mod tests {
    use super::*;

    #[test]
    fn every_vertex_fits_inside_its_own_stride() {
        let stride = std::mem::size_of::<Vertex>() as wgpu::BufferAddress;

        assert_eq!(stride, 24);
        for attribute in ATTRIBUTES {
            assert!(attribute.offset + attribute.format.size() <= stride);
        }
    }
}
