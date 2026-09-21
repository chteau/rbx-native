//! The adornment pass's pipelines and vertex layouts: a solid one, a
//! screen-expanded line one and a textured one, each in a depth-tested and
//! an always-on-top variant.
//!
//! Built together the first time a place actually holds an adornment (see
//! [`super::Adornments`]), so a place with none compiles none of them.

use super::{ImageVertex, LineVertex, Vertex};
use crate::renderer::pipeline::{self, Surface, Target};

const SOLID_SHADER: &str = include_str!("../adornment.wgsl");
const LINE_SHADER: &str = include_str!("../adornment_line.wgsl");
const IMAGE_SHADER: &str = include_str!("../adornment_image.wgsl");

pub(super) struct Gpu {
    solid: [wgpu::RenderPipeline; 2],
    line: [wgpu::RenderPipeline; 2],
    image: [wgpu::RenderPipeline; 2],
}

impl Gpu {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        image_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let frame = pipeline::frame_layout(device);
        let both = |label: &str,
                    shader: &str,
                    buffer: fn() -> wgpu::VertexBufferLayout<'static>,
                    layouts: &[Option<&wgpu::BindGroupLayout>]| {
            [false, true]
                .map(|on_top| build(device, target, label, shader, buffer(), layouts, on_top))
        };

        Gpu {
            solid: both(
                "rbxview adornment",
                SOLID_SHADER,
                Vertex::layout,
                &[Some(&frame)],
            ),
            line: both(
                "rbxview adornment line",
                LINE_SHADER,
                LineVertex::layout,
                &[Some(&frame)],
            ),
            image: both(
                "rbxview adornment image",
                IMAGE_SHADER,
                ImageVertex::layout,
                &[Some(&frame), Some(image_layout)],
            ),
        }
    }

    pub(super) fn solid(&self, on_top: bool) -> &wgpu::RenderPipeline {
        &self.solid[usize::from(on_top)]
    }

    pub(super) fn line(&self, on_top: bool) -> &wgpu::RenderPipeline {
        &self.line[usize::from(on_top)]
    }

    pub(super) fn image(&self, on_top: bool) -> &wgpu::RenderPipeline {
        &self.image[usize::from(on_top)]
    }
}

/// One adornment pipeline: blended, unculled, depth-tested or — `on_top` —
/// drawn over everything.
fn build(
    device: &wgpu::Device,
    target: Target,
    label: &str,
    shader: &str,
    buffer: wgpu::VertexBufferLayout<'_>,
    layouts: &[Option<&wgpu::BindGroupLayout>],
    on_top: bool,
) -> wgpu::RenderPipeline {
    pipeline::surface(
        device,
        target,
        &Surface {
            // An adornment is a thin overlay a camera may well be
            // standing inside; culling would leave a hole where its
            // near face should be.
            cull: None,
            // Blended, and writing no depth: an adornment is drawn
            // over the scene, not part of it.
            translucent: true,
            // Reversed-Z, so `GreaterEqual` is the ordinary "in
            // front of, or exactly on, what is already there" test —
            // the tie matters for a `SurfaceSelection` slab, which
            // lies on the very surface it highlights. `Always` is
            // what `AlwaysOnTop` means.
            compare: if on_top {
                wgpu::CompareFunction::Always
            } else {
                wgpu::CompareFunction::GreaterEqual
            },
            ..Surface::new(label, shader, layouts, &[Some(buffer)])
        },
    )
}

const SOLID_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32, 4 => Float32x4];
const IMAGE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];

impl Vertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &SOLID_ATTRIBUTES,
        }
    }
}

impl LineVertex {
    /// Shared with `renderer::lines`, whose lines are built by the same
    /// `geometry::line`.
    pub(in crate::renderer) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &LINE_ATTRIBUTES,
        }
    }
}

impl ImageVertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &IMAGE_ATTRIBUTES,
        }
    }
}
