//! The Properties-panel fast paths `Renderer::patch_instance` cannot serve: a
//! part drawn as a resolved file mesh rather than as a boxed instance, and
//! the effects (`ParticleEmitter`/`Beam`/`Trail`) drawn from static
//! definitions rather than from any instance buffer at all.

use super::Renderer;
use crate::scene::{EffectKind, ResolvedInstance, Scene};

impl Renderer {
    /// [`Renderer::patch_instance`] for a [`ResolvedInstance`]: its colour-pass
    /// copy, and its shadow caster if it has one (see
    /// `crate::scene::Scene::patch_mesh_instance`, which already refused
    /// anything that would cross a batch).
    ///
    /// `false` means `instance.referent` was not found where the scene's own
    /// batch check said it would be — the same defensive fallback as
    /// `patch_instance`'s, not a case a caller needs to reason about.
    pub(crate) fn patch_mesh_instance(
        &mut self,
        queue: &wgpu::Queue,
        instance: &ResolvedInstance,
    ) -> bool {
        if !self.filemesh.patch(queue, instance) {
            return false;
        }
        !instance.casts_shadow
            || self
                .shadows
                .patch_mesh_caster(queue, instance.referent, instance.model)
    }

    /// Hands the renderer `scene`'s freshly re-planned effects of `kind` (see
    /// `crate::scene::Scene::replan_effect`), keeping every texture already
    /// uploaded and every running simulation or recorder that still applies
    /// — see each pass's own `replace` for exactly what survives.
    ///
    /// `false` when a definition names a texture this renderer never tried
    /// to download, which only a full reload fetches; the caller falls back.
    pub(crate) fn patch_effect(&mut self, kind: EffectKind, scene: &Scene) -> bool {
        match kind {
            EffectKind::Particles => self.particles.replace(scene.particle_emitters()),
            EffectKind::Beams => self.beams.replace(scene.beams()),
            EffectKind::Trails => self.trails.replace(scene.trails()),
        }
    }
}
