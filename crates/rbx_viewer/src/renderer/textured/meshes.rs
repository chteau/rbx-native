//! The real meshes decals land on: a plain position and normal copy of each
//! mesh asset a `Decal`/`Texture` is projected onto (see `MeshFace`), drawn
//! through the decal pipeline exactly like a unit shape.

use std::collections::{HashMap, HashSet};

use wgpu::util::DeviceExt;

use super::super::mesh::Vertex;
use crate::scene::Resolved;
use crate::textures::MeshFace;

pub(super) struct DecalMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl DecalMesh {
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: u32) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..instances);
    }
}

/// Every mesh a decal is projected onto, by [`MeshFace::key`].
#[derive(Default)]
pub(super) struct DecalMeshes(HashMap<u64, DecalMesh>);

impl DecalMeshes {
    pub(super) fn get(&self, key: u64) -> Option<&DecalMesh> {
        self.0.get(&key)
    }

    /// Builds `key`'s mesh from the scene's resolved meshes unless it is
    /// here already. `false` when the scene holds no such mesh.
    pub(super) fn ensure(&mut self, device: &wgpu::Device, key: u64, resolved: &Resolved) -> bool {
        if self.0.contains_key(&key) {
            return true;
        }
        let Some(mesh) = resolved
            .meshes
            .iter()
            .find_map(|(asset, mesh)| (MeshFace::key_of(asset) == key).then_some(mesh))
        else {
            return false;
        };
        let vertices: Vec<Vertex> = mesh
            .vertices
            .iter()
            .map(|vertex| Vertex::new(vertex.position, vertex.normal))
            .collect();
        let indices = mesh.lod0();
        let decal = DecalMesh {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview decal mesh vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview decal mesh indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: indices.len() as u32,
        };
        self.0.insert(key, decal);
        true
    }

    /// Keeps only the meshes in `keys`, building any that are missing.
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        keys: &HashSet<u64>,
        resolved: &Resolved,
    ) {
        self.0.retain(|key, _| keys.contains(key));
        for &key in keys {
            self.ensure(device, key, resolved);
        }
    }
}
