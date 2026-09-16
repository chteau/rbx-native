//! The Properties-panel fast paths `Renderer::sync_instance` cannot serve: a
//! part drawn as a resolved file mesh rather than as a boxed instance, and
//! the effects (`ParticleEmitter`/`Beam`/`Trail`) drawn from static
//! definitions rather than from any instance buffer at all.

use rbx_dom::Ref;

use super::Renderer;
use crate::scene::{EffectKind, Resolved, ResolvedInstance, Scene};

impl Renderer {
    /// [`Renderer::sync_instance`] for a [`ResolvedInstance`]: its colour-pass
    /// copy, opaque or blended, and its shadow caster if it has one — each
    /// moved between batches if the edit crossed one (see
    /// `crate::scene::Scene::patch_mesh_instance` for which edits get here).
    ///
    /// `false` means a batch the instance now belongs in would need a mesh
    /// or texture `resolved` never downloaded — the scene's own check already
    /// refused that, so this is a defensive fallback rather than a case a
    /// caller needs to reason about.
    pub(crate) fn sync_mesh_instance(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
        instance: &ResolvedInstance,
    ) -> bool {
        self.filemesh.sync(device, queue, resolved, instance)
            && self
                .shadows
                .sync_mesh_caster(device, queue, resolved, instance)
    }

    /// Drops a resolved mesh instance the scene stopped drawing — its part
    /// turned fully transparent (see `Scene::patch_mesh_instance`'s
    /// `MeshPatch::Removed`). A no-op for a referent no batch holds.
    pub(crate) fn remove_mesh_instance(&mut self, queue: &wgpu::Queue, referent: Ref) {
        self.filemesh.remove(queue, referent);
        self.shadows.remove_mesh_caster(queue, referent);
    }

    /// Hands the renderer `scene`'s freshly re-planned effects of `kind` (see
    /// `crate::scene::Scene::replan_effect`), keeping every texture already
    /// uploaded and every running simulation or recorder that still applies
    /// — see each pass's own `replace` for exactly what survives.
    ///
    /// Always serves the edit: a definition naming a texture this renderer has
    /// no upload for draws that effect's own fallback until the loader lands
    /// one, which is what each `replace` documents.
    pub(crate) fn patch_effect(&mut self, kind: EffectKind, scene: &Scene) {
        match kind {
            EffectKind::Particles => self.particles.replace(scene.particle_emitters()),
            EffectKind::Beams => self.beams.replace(scene.beams()),
            EffectKind::Trails => self.trails.replace(scene.trails()),
        }
    }
}
