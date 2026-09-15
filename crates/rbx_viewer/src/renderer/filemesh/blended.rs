//! The blended half of [`FileMeshes`]: batches whose instances have to be
//! re-sorted against the camera every frame, so they live on the CPU and
//! are re-uploaded whole rather than patched slot by slot.

use glam::Vec3;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use super::{Geometry, GroupKey, InstanceRaw};
use crate::scene::Resolved;

/// A translucent batch keeps its instances on the CPU as well: blending is
/// order-dependent, so both the instances within a batch and the batches
/// themselves are re-sorted every frame.
pub(super) struct Blended {
    pub(super) key: GroupKey,
    pub(super) geometry: Geometry,
    /// Rewritten from `items` every frame by [`FileMeshes::prepare`], so it
    /// only has to be big enough: `capacity` items, at least `items.len()`.
    pub(super) instances: wgpu::Buffer,
    pub(super) capacity: usize,
    /// Each instance's referent, world-space centre and GPU record, in the
    /// order [`FileMeshes::prepare`] last sorted them — the referent is what
    /// an edit finds an item by, since that order changes.
    pub(super) items: Vec<(Ref, Vec3, InstanceRaw)>,
    /// This frame's distance to the furthest instance in the batch.
    pub(super) depth: f32,
}

impl Blended {
    pub(super) fn new(
        device: &wgpu::Device,
        key: GroupKey,
        geometry: Geometry,
        items: Vec<(Ref, Vec3, InstanceRaw)>,
    ) -> Self {
        let instances: Vec<InstanceRaw> = items.iter().map(|(_, _, raw)| *raw).collect();
        Blended {
            key,
            geometry,
            instances: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview filemesh instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            capacity: items.len(),
            items,
            depth: 0.0,
        }
    }
}

/// Whether a `SurfaceAppearance`'s colour map carries alpha, which sends every
/// instance wearing it to the blended pass whatever its own `Transparency`.
/// Scanning the map is a whole-image pass, so this runs once per batch
/// rather than once per instance.
pub(super) fn blends(resolved: &Resolved, key: &GroupKey) -> bool {
    key.appearance
        .and_then(|index| resolved.appearances.get(index))
        .is_some_and(|set| set.is_translucent(&resolved.images))
}
