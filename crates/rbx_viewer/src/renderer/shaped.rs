//! The fully opaque parts of a scene, grouped by the unit shape they instance.
//! Anything with a `Transparency` above zero goes to `super::translucent`
//! instead.
//!
//! Every kind — the cube included — goes through the same lit pipeline: the
//! vertex and instance layouts are identical, so only the mesh and the instance
//! buffer change between groups.

use glam::Vec3;

use super::cull::{visible_runs, MainCull};
use super::geometry::Meshes;
use super::instance::InstanceRaw;
use super::slots::keyed::Keyed;
use super::slots::Roster;
use crate::scene::{of_part, Part, PartId, ShapeKind};

/// One world-space bounding sphere (center, radius) per instance, kept
/// beside the GPU record — what [`Shaped::draw`]'s cull test checks each one
/// against without ever touching the buffer itself.
type Sphere = (Vec3, f32);

/// A scene's opaque parts, one batch per shape kind, ready to draw.
pub(super) struct Shaped {
    batches: Keyed<ShapeKind, (), InstanceRaw, Sphere, PartId>,
}

impl Shaped {
    pub(super) fn new(device: &wgpu::Device, parts: &[Part]) -> Self {
        let mut batches = Keyed::new("rbxview part instances");
        for kind in kinds(parts) {
            let roster = Roster::from_iter(
                parts
                    .iter()
                    .filter(|part| part.kind == kind && belongs(part))
                    .map(|part| (part.id, InstanceRaw::from_part(part), sphere(part))),
            );
            batches.add_group(device, kind, (), roster);
        }

        Shaped { batches }
    }

    /// Draws every batch, skipping whatever `cull` rules out before it ever
    /// costs a draw call: each batch is split into the maximal runs of
    /// consecutive visible instances (see `super::cull::visible_runs`) and
    /// only those are drawn, in the buffer's existing order — reordering it
    /// would break the per-instance slots [`Shaped::sync`] writes into.
    ///
    /// The caller must already have the lit pipeline and the camera bind
    /// group bound.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        meshes: &Meshes,
        cull: &MainCull<'_>,
    ) {
        for batch in self.batches.groups() {
            let Some(mesh) = meshes.get(batch.key) else {
                continue;
            };
            let runs = visible_runs(batch.slots.count(), |index| {
                let (center, radius) = batch.slots.side(index);
                cull.visible(center, radius)
            });
            if runs.is_empty() {
                continue;
            }

            pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
            for run in runs {
                mesh.draw_range(pass, run);
            }
        }
    }

    /// Brings this pass in line with one edited part: rewritten in place if
    /// it is still opaque and still the same shape, moved to its new shape's
    /// batch if not, dropped if it turned translucent or invisible, and added
    /// if it just became opaque. `part.kind`'s mesh must already be in
    /// [`Meshes`] (see `Meshes::ensure`) for the new batch to draw.
    pub(super) fn sync(&mut self, device: &wgpu::Device, part: &Part) {
        let wanted = belongs(part).then(|| (part.kind, InstanceRaw::from_part(part), sphere(part)));
        // A unit shape's batch needs no payload, so a new one can always be
        // made — the `false` case never happens here.
        self.batches.sync(device, part.id, wanted, |_| Some(()));
    }

    /// Takes one box out of whichever batch holds it — it stopped drawing
    /// as one, or is gone. A no-op for an id no batch holds.
    pub(super) fn remove(&mut self, id: PartId) {
        self.batches.remove(id);
    }

    /// Uploads what the edits since the last frame owe the buffers — see
    /// `slots::Slots::flush`.
    pub(super) fn flush(&mut self, queue: &wgpu::Queue) {
        self.batches.flush(queue);
    }
}

fn belongs(part: &Part) -> bool {
    part.is_drawn() && !part.is_translucent()
}

fn sphere(part: &Part) -> Sphere {
    let extent = of_part(part);
    (extent.center(), extent.radius())
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
