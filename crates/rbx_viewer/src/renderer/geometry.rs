//! The unit meshes every pass instances, uploaded once and shared.
//!
//! A part and the decals painted on it are drawn from the *same* geometry — the
//! decal pass reprojects the part's own surface rather than floating a quad in
//! front of it — so both take their vertex and index buffers from here.

use wgpu::util::DeviceExt;

use super::mesh::{self, Vertex};
use crate::scene::ShapeKind;
use crate::shapes::{self, MeshData};

/// One unit shape's vertex and index buffers.
pub(super) struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl Mesh {
    /// Binds this mesh at slot 0 and draws `instances` copies of it; the caller
    /// owns slot 1 (the instance buffer) and the pipeline.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: u32) {
        self.draw_range(pass, 0..instances);
    }

    /// Draws only part of the bound instance buffer. The translucent pass sorts
    /// every shape into one buffer and needs to draw slices of it in order.
    pub(super) fn draw_range(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        instances: std::ops::Range<u32>,
    ) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.index_count, 0, instances);
    }
}

/// The unit meshes a scene needs, one per [`ShapeKind`] in use.
///
/// A kind nobody instances is never built, which is most of the six in most
/// places.
pub(super) struct Meshes {
    entries: Vec<(ShapeKind, Mesh)>,
}

impl Meshes {
    pub(super) fn new(device: &wgpu::Device, kinds: impl IntoIterator<Item = ShapeKind>) -> Self {
        let mut entries: Vec<(ShapeKind, Mesh)> = Vec::new();
        for kind in kinds {
            if entries.iter().all(|(known, _)| *known != kind) {
                entries.push((kind, build(device, kind)));
            }
        }

        Meshes { entries }
    }

    /// Builds `kind`'s mesh if no part instanced it when the scene loaded —
    /// what a `Part` edited from a box into the place's first ball needs
    /// before any batch can draw it. Nothing to do for a kind already here.
    pub(super) fn ensure(&mut self, device: &wgpu::Device, kind: ShapeKind) {
        if self.get(kind).is_none() {
            self.entries.push((kind, build(device, kind)));
        }
    }

    /// `None` only for a kind that was not among the ones handed to [`new`]
    /// or [`Meshes::ensure`].
    pub(super) fn get(&self, kind: ShapeKind) -> Option<&Mesh> {
        self.entries
            .iter()
            .find(|(known, _)| *known == kind)
            .map(|(_, mesh)| mesh)
    }
}

// These low-poly shapes stay well under `u16::MAX` vertices, so a 16-bit index
// buffer is enough for all of them.
fn build(device: &wgpu::Device, kind: ShapeKind) -> Mesh {
    let (vertices, indices) = match kind {
        ShapeKind::Box => mesh::cube(),
        _ => from_data(shape_data(kind)),
    };

    Mesh {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview shape vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        index_count: indices.len() as u32,
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview shape indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
    }
}

fn shape_data(kind: ShapeKind) -> MeshData {
    match kind {
        ShapeKind::Ball => shapes::sphere(),
        ShapeKind::CylinderX => shapes::cylinder_x(),
        ShapeKind::CylinderY => shapes::cylinder_y(),
        ShapeKind::Wedge => shapes::wedge(),
        ShapeKind::CornerWedge => shapes::corner_wedge(),
        ShapeKind::Truss {
            axis,
            segments,
            style,
        } => shapes::oriented(shapes::truss(segments, style), axis),
        // `build` sends Box to the hand-written cube, which keeps its flat
        // per-face normals rather than going through a generator.
        ShapeKind::Box => unreachable!("the cube is not a generated shape"),
    }
}

fn from_data(data: MeshData) -> (Vec<Vertex>, Vec<u16>) {
    let vertices = data
        .positions
        .iter()
        .zip(&data.normals)
        .map(|(&position, &normal)| Vertex::new(position, normal))
        .collect();
    let indices = data
        .indices
        .iter()
        .map(|&index| u16::try_from(index).unwrap_or(0))
        .collect();

    (vertices, indices)
}
