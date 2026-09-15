//! The fully opaque parts of a scene, grouped by the unit shape they instance.
//! Anything with a `Transparency` above zero goes to `super::translucent`
//! instead.
//!
//! Every kind — the cube included — goes through the same lit pipeline: the
//! vertex and instance layouts are identical, so only the mesh and the instance
//! buffer change between groups.

use std::collections::HashMap;

use glam::Vec3;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use super::cull::{visible_runs, MainCull};
use super::geometry::Meshes;
use super::instance::InstanceRaw;
use crate::scene::{of_part, Part, ShapeKind};

/// One shape kind's instances, in the fixed order they were built in — the
/// order [`Shaped::patch`]'s offsets and this batch's own `bounds` both index
/// into.
struct Batch {
    kind: ShapeKind,
    instances: wgpu::Buffer,
    instance_count: u32,
    /// One world-space bounding sphere (center, radius) per instance,
    /// parallel to `instances` — what [`Shaped::draw`]'s cull test checks
    /// each one against without ever touching the GPU buffer itself.
    bounds: Vec<(Vec3, f32)>,
}

/// A scene's parts, ready to draw.
pub(super) struct Shaped {
    batches: Vec<Batch>,
    /// Where each caster's instance sits — its batch and its offset within
    /// it — so a single-instance edit (see [`Shaped::patch`]) can write
    /// straight into the buffer instead of rebuilding it.
    part_index: HashMap<Ref, (ShapeKind, u32)>,
}

impl Shaped {
    pub(super) fn new(device: &wgpu::Device, parts: &[Part]) -> Self {
        let mut batches: Vec<Batch> = Vec::new();
        let mut part_index = HashMap::new();
        for kind in kinds(parts) {
            let members: Vec<&Part> = parts
                .iter()
                .filter(|part| part.kind == kind && part.is_drawn() && !part.is_translucent())
                .collect();
            for (offset, part) in members.iter().enumerate() {
                part_index.insert(part.referent, (kind, offset as u32));
            }
            let instances: Vec<InstanceRaw> = members
                .iter()
                .copied()
                .map(InstanceRaw::from_part)
                .collect();
            let bounds = members
                .iter()
                .map(|part| {
                    let extent = of_part(part);
                    (extent.center(), extent.radius())
                })
                .collect();

            batches.push(Batch {
                kind,
                instance_count: instances.len() as u32,
                bounds,
                instances: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rbxview part instances"),
                    contents: bytemuck::cast_slice(&instances),
                    // Written afterwards only by `Shaped::patch`, one instance
                    // at a time — see its doc comment.
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
            });
        }

        Shaped {
            batches,
            part_index,
        }
    }

    /// Draws every batch, skipping whatever `cull` rules out before it ever
    /// costs a draw call: each batch is split into the maximal runs of
    /// consecutive visible instances (see `super::cull::visible_runs`) and
    /// only those are drawn, in the buffer's existing order — reordering it
    /// would break [`Shaped::patch`]'s fixed per-instance offsets.
    ///
    /// The caller must already have the lit pipeline and the camera bind
    /// group bound.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        meshes: &Meshes,
        cull: &MainCull<'_>,
    ) {
        for batch in &self.batches {
            let Some(mesh) = meshes.get(batch.kind) else {
                continue;
            };
            let runs = visible_runs(batch.instance_count, |index| {
                let (center, radius) = batch.bounds[index as usize];
                cull.visible(center, radius)
            });
            if runs.is_empty() {
                continue;
            }

            pass.set_vertex_buffer(1, batch.instances.slice(..));
            for run in runs {
                mesh.draw_range(pass, run);
            }
        }
    }

    /// Rewrites one instance in place — `part` must still land in the same
    /// batch it was built into (see `crate::scene::Scene::patch_part`'s bucket
    /// check). `false` when `referent` was never one of this batch's opaque
    /// instances (suppressed, translucent, or not drawn at all), which is the
    /// caller's cue to fall back to a full reload instead.
    pub(super) fn patch(&mut self, queue: &wgpu::Queue, part: &Part) -> bool {
        let Some(&(kind, offset)) = self.part_index.get(&part.referent) else {
            return false;
        };
        if kind != part.kind {
            return false;
        }
        let Some(batch) = self.batches.iter_mut().find(|batch| batch.kind == kind) else {
            return false;
        };

        let stride = std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress;
        queue.write_buffer(
            &batch.instances,
            u64::from(offset) * stride,
            bytemuck::bytes_of(&InstanceRaw::from_part(part)),
        );
        let extent = of_part(part);
        batch.bounds[offset as usize] = (extent.center(), extent.radius());
        true
    }
}

/// Every shape kind the scene still stands for, in first-seen order.
///
/// Also what [`Meshes`] is built from, so a kind present only as a suppressed
/// part costs no GPU buffer at all. Invisible parts (`Transparency` 1) are
/// deliberately counted: nothing draws them, but a `Decal` pinned to one is
/// still painted on that same unit mesh.
pub(super) fn kinds(parts: &[Part]) -> Vec<ShapeKind> {
    let mut kinds: Vec<ShapeKind> = Vec::new();
    for part in parts.iter().filter(|part| !part.is_suppressed()) {
        if !kinds.contains(&part.kind) {
            kinds.push(part.kind);
        }
    }
    kinds
}
