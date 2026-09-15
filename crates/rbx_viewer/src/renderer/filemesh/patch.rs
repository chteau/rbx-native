//! The Properties-panel fast path for one resolved mesh instance — the
//! file-mesh counterpart of `renderer::shaped::Shaped::patch` and
//! `renderer::translucent::Translucent::patch`.

use super::{center, raw, FileMeshes, InstanceRaw};
use crate::scene::ResolvedInstance;

impl FileMeshes {
    /// Rewrites one instance in place — `instance` must still belong to the
    /// batch it was built into (see `crate::scene::Scene::patch_mesh_instance`,
    /// whose batch check guarantees it). An opaque instance is written
    /// straight into its buffer; a blended one only has its CPU item
    /// replaced, since [`FileMeshes::prepare`] re-sorts and re-uploads those
    /// before every draw anyway.
    ///
    /// `false` when `instance.referent` is in neither index (never resolved,
    /// or dropped as invisible) — the caller's cue to fall back to a full
    /// reload.
    pub(in crate::renderer) fn patch(
        &mut self,
        queue: &wgpu::Queue,
        instance: &ResolvedInstance,
    ) -> bool {
        if let Some(&(batch, offset)) = self.opaque_index.get(&instance.referent) {
            let Some(batch) = self.opaque.get(batch) else {
                return false;
            };
            let stride = std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress;
            queue.write_buffer(
                &batch.instances,
                u64::from(offset) * stride,
                bytemuck::bytes_of(&raw(instance)),
            );
            return true;
        }
        if let Some(&batch) = self.blended_index.get(&instance.referent) {
            let Some(blended) = self.blended.get_mut(batch) else {
                return false;
            };
            let Some(item) = blended
                .items
                .iter_mut()
                .find(|(referent, ..)| *referent == instance.referent)
            else {
                return false;
            };
            *item = (instance.referent, center(instance), raw(instance));
            return true;
        }
        false
    }
}
