//! The scene's shadow casters, as the buffers the depth pass instances.
//!
//! Split out from [`super::Shadows`] because they are the one part of the map that
//! a quality level never touches: the map's size changes with the level, the
//! casters in it do not.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::scene::{of_part, of_transform, Part, Resolved, Scene, ShapeKind};

pub(super) const POSITION_ATTRIBUTE: [wgpu::VertexAttribute; 1] =
    wgpu::vertex_attr_array![0 => Float32x3];
pub(super) const CASTER_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
];

/// One caster: where it stands, and nothing else.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct CasterRaw {
    model: [[f32; 4]; 4],
}

/// Every caster of one unit shape, drawn against [`Meshes`]' own buffers.
pub(super) struct ShapeBatch {
    pub(super) kind: ShapeKind,
    pub(super) instances: wgpu::Buffer,
    pub(super) count: u32,
    /// One world-space bounding sphere per caster, parallel to `instances` —
    /// what the sun pass's own, deliberately wider visibility test
    /// (`Fit::visible`) checks each one against; see `Shadows::draw_casters`.
    pub(super) bounds: Vec<(Vec3, f32)>,
}

/// Every caster of one file mesh, with that mesh's positions and indices.
pub(super) struct MeshBatch {
    pub(super) vertices: wgpu::Buffer,
    pub(super) indices: wgpu::Buffer,
    pub(super) index_count: u32,
    pub(super) instances: wgpu::Buffer,
    pub(super) count: u32,
}

/// Where a caster's instance sits — its batch and its offset within it — for
/// a single-instance edit (see [`patch`]) to write straight into the buffer
/// instead of rebuilding it. Keyed separately from `Shaped`'s own index: a
/// caster batch is filtered by `casts_shadow()` alone, not by
/// drawn/translucent too, so the same part can land at a different offset in
/// each.
pub(super) type CasterIndex = HashMap<Ref, (ShapeKind, u32)>;

/// The casters among the scene's parts, grouped by unit shape. Translucent
/// parts are in: Roblox casts a full shadow from anything below `Transparency`
/// 1, which is why the reference capture's 0.7-transparent slab still has one.
pub(super) fn shape_batches(
    device: &wgpu::Device,
    scene: &Scene,
) -> (Vec<ShapeBatch>, CasterIndex) {
    let mut batches: Vec<ShapeBatch> = Vec::new();
    let mut index = CasterIndex::new();
    for kind in super::super::shaped::kinds(scene.parts()) {
        let members: Vec<&Part> = scene
            .parts()
            .iter()
            .filter(|part| part.kind == kind && part.casts_shadow())
            .collect();
        if members.is_empty() {
            continue;
        }
        for (offset, part) in members.iter().enumerate() {
            index.insert(part.referent, (kind, offset as u32));
        }
        let instances: Vec<CasterRaw> = members
            .iter()
            .map(|part| CasterRaw {
                model: part.transform.to_cols_array_2d(),
            })
            .collect();
        let bounds = members
            .iter()
            .map(|part| {
                let extent = of_part(part);
                (extent.center(), extent.radius())
            })
            .collect();

        batches.push(ShapeBatch {
            kind,
            count: instances.len() as u32,
            bounds,
            instances: buffer(device, bytemuck::cast_slice(&instances)),
        });
    }
    (batches, index)
}

/// Rewrites one caster's transform in place. `false` when `referent` is not
/// among `batches`' own casters, or landed in a different shape's batch than
/// `index` says — either is the caller's cue to fall back to a full reload.
pub(super) fn patch(
    queue: &wgpu::Queue,
    batches: &mut [ShapeBatch],
    index: &CasterIndex,
    referent: Ref,
    kind: ShapeKind,
    model: [[f32; 4]; 4],
) -> bool {
    let Some(&(found_kind, offset)) = index.get(&referent) else {
        return false;
    };
    if found_kind != kind {
        return false;
    }
    let Some(batch) = batches.iter_mut().find(|batch| batch.kind == kind) else {
        return false;
    };

    let stride = std::mem::size_of::<CasterRaw>() as wgpu::BufferAddress;
    queue.write_buffer(
        &batch.instances,
        u64::from(offset) * stride,
        bytemuck::bytes_of(&CasterRaw { model }),
    );
    let extent = of_transform(Mat4::from_cols_array_2d(&model));
    batch.bounds[offset as usize] = (extent.center(), extent.radius());
    true
}

/// The casters among the resolved file meshes, one batch per mesh asset. A
/// mesh's skin is irrelevant to a depth pass, so the (mesh, skin) split the
/// colour pass batches by collapses back to one batch here.
pub(super) fn mesh_batches(device: &wgpu::Device, resolved: &Resolved) -> Vec<MeshBatch> {
    let mut order: Vec<AssetRef> = Vec::new();
    let mut grouped: HashMap<AssetRef, Vec<CasterRaw>> = HashMap::new();
    for instance in resolved.instances.iter().filter(|i| i.casts_shadow) {
        let casters = grouped.entry(instance.mesh.clone()).or_insert_with(|| {
            order.push(instance.mesh.clone());
            Vec::new()
        });
        casters.push(CasterRaw {
            model: instance.model.to_cols_array_2d(),
        });
    }

    order
        .into_iter()
        .filter_map(|reference| {
            let mesh = resolved.meshes.get(&reference)?;
            let instances = grouped.get(&reference)?;
            let positions: Vec<[f32; 3]> =
                mesh.vertices.iter().map(|vertex| vertex.position).collect();
            let indices = mesh.lod0();

            Some(MeshBatch {
                vertices: buffer(device, bytemuck::cast_slice(&positions)),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rbxview shadow indices"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: indices.len() as u32,
                instances: buffer(device, bytemuck::cast_slice(instances)),
                count: instances.len() as u32,
            })
        })
        .collect()
}

fn buffer(device: &wgpu::Device, contents: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rbxview shadow casters"),
        contents,
        // Written afterwards only by `patch`, one instance at a time.
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    })
}
