//! The scene's shadow casters, as the buffers the depth pass instances.
//!
//! Split out from [`super::Shadows`] because they are the one part of the map that
//! a quality level never touches: the map's size changes with the level, the
//! casters in it do not.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::super::rebuild::take_spare;
use super::super::slots::keyed::Keyed;
use super::super::slots::Roster;
use crate::scene::{of_part, Part, PartId, Resolved, ResolvedInstance, Scene, ShapeKind};

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

/// One world-space bounding sphere per caster, beside its record — what the
/// sun pass's own, deliberately wider visibility test (`Fit::visible`) checks
/// each one against; see `Shadows::draw_casters`.
pub(super) type Sphere = (Vec3, f32);

/// The casters of every unit shape, drawn against [`super::Meshes`]' own
/// buffers. Indexed separately from `Shaped`'s own batches: a caster batch
/// is filtered by `casts_shadow()` alone, not by drawn/translucent too, so
/// the same part lands at a different slot in each.
pub(super) type ShapeBatches = Keyed<ShapeKind, (), CasterRaw, Sphere, PartId>;

/// A file mesh's positions and indices, the payload of one [`MeshBatches`]
/// group. A mesh's skin is irrelevant to a depth pass, so the (mesh, skin)
/// split the colour pass batches by collapses back to one batch per mesh.
pub(in crate::renderer) struct MeshGeometry {
    pub(in crate::renderer) vertices: wgpu::Buffer,
    pub(in crate::renderer) indices: wgpu::Buffer,
    pub(in crate::renderer) index_count: u32,
}

pub(super) type MeshBatches = Keyed<AssetRef, MeshGeometry, CasterRaw, ()>;

/// The casters among the scene's parts, grouped by unit shape. Translucent
/// parts are in: Roblox casts a full shadow from anything below `Transparency`
/// 1, which is why the reference capture's 0.7-transparent slab still has one.
pub(super) fn shape_batches(device: &wgpu::Device, scene: &Scene) -> ShapeBatches {
    let mut batches = Keyed::new("rbxview shadow casters");
    for kind in super::super::shaped::kinds(scene.parts()) {
        let roster = Roster::from_iter(
            scene
                .parts()
                .iter()
                .filter(|part| part.kind == kind && part.casts_shadow())
                .map(|part| (part.id, raw(part.transform), sphere(part))),
        );
        if roster.len() > 0 {
            batches.add_group(device, kind, (), roster);
        }
    }
    batches
}

/// Brings the shape casters in line with one edited part — see
/// `renderer::shaped::Shaped::sync` for the same four outcomes.
pub(super) fn sync_shape(device: &wgpu::Device, batches: &mut ShapeBatches, part: &Part) {
    let wanted = part
        .casts_shadow()
        .then(|| (part.kind, raw(part.transform), sphere(part)));
    batches.sync(device, part.id, wanted, |_| Some(()));
}

/// The casters among the resolved file meshes, one batch per mesh asset.
///
/// `spare` is the previous scene's geometry by mesh asset (see
/// `Shadows::rebuild`), taken over wherever the same mesh casts again rather
/// than copied out of the mesh and uploaded a second time; empty for a first
/// build.
pub(super) fn mesh_batches(
    device: &wgpu::Device,
    resolved: &Resolved,
    spare: &mut Vec<(AssetRef, MeshGeometry)>,
) -> MeshBatches {
    let mut batches = Keyed::new("rbxview shadow casters");
    let mut order: Vec<AssetRef> = Vec::new();
    for instance in resolved.instances.iter().filter(|i| i.casts_shadow) {
        if !order.contains(&instance.mesh) {
            order.push(instance.mesh.clone());
        }
    }
    for reference in order {
        let Some(geometry) = take_spare(spare, &reference, |_| true)
            .or_else(|| geometry(device, resolved, &reference))
        else {
            continue;
        };
        let roster = Roster::from_iter(
            resolved
                .instances
                .iter()
                .filter(|i| i.casts_shadow && i.mesh == reference)
                .map(|i| (i.referent, raw(i.model), ())),
        );
        batches.add_group(device, reference, geometry, roster);
    }
    batches
}

/// [`sync_shape`] for a resolved file mesh. `false` when the instance now
/// casts through a mesh `resolved` never downloaded — which
/// `Scene::resync_part` already refuses, so this is a defensive
/// answer rather than a documented case.
pub(super) fn sync_mesh(
    device: &wgpu::Device,
    batches: &mut MeshBatches,
    resolved: &Resolved,
    instance: &ResolvedInstance,
) -> bool {
    let wanted = instance
        .casts_shadow
        .then(|| (instance.mesh.clone(), raw(instance.model), ()));
    batches.sync(device, instance.referent, wanted, |mesh| {
        geometry(device, resolved, mesh)
    })
}

/// A mesh asset's positions and indices, uploaded on their own: the only
/// thing a pass that draws silhouettes rather than surfaces needs of it.
/// Shared with `renderer::highlight`, which re-draws geometry into a mask for
/// the same reason this one re-draws it into a depth map.
pub(in crate::renderer) fn geometry(
    device: &wgpu::Device,
    resolved: &Resolved,
    mesh: &AssetRef,
) -> Option<MeshGeometry> {
    let mesh = resolved.meshes.get(mesh)?;
    let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|vertex| vertex.position).collect();
    let indices = mesh.lod0();
    Some(MeshGeometry {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview shadow vertices"),
            contents: bytemuck::cast_slice(&positions),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview shadow indices"),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: indices.len() as u32,
    })
}

fn raw(model: Mat4) -> CasterRaw {
    CasterRaw {
        model: model.to_cols_array_2d(),
    }
}

fn sphere(part: &Part) -> Sphere {
    let extent = of_part(part);
    (extent.center(), extent.radius())
}
