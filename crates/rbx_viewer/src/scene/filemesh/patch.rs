//! The file-mesh half of `Scene::patch_mesh_instance`: re-reading one
//! `MeshPart`/`SpecialMesh` entry off the DOM and rebuilding its
//! [`ResolvedInstance`] against meshes and images that already downloaded,
//! without going anywhere near the network.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{from_mesh_part, from_special_mesh_child, Entry, Resolved, ResolvedInstance};
use crate::scene::material::Catalog;

/// The single-instance counterpart of [`super::plan`]: the entry `referent`
/// would get from a fresh plan, or `None` when it is not file-mesh-backed at
/// all (a plain `Part`, or a `MeshPart` whose `MeshId` was just emptied).
pub(crate) fn replan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    from_mesh_part(dom, database, referent, materials)
        .or_else(|| from_special_mesh_child(dom, database, referent, materials))
}

impl Entry {
    /// Whether a fresh [`super::resolve`] would drop this entry as fully
    /// transparent — told apart from [`Entry::patched`]'s other `None`s, since
    /// an invisible instance is removed in place while the rest need a reload.
    pub(crate) fn is_invisible(&self) -> bool {
        self.alpha <= 0.0
    }

    /// The instance [`super::resolve`] would build for this entry against
    /// `resolved`'s already-downloaded assets — `None` wherever a fresh
    /// resolution would drop it (its mesh never downloaded, or it is now
    /// fully transparent) or would need something only a full reload
    /// downloads: a `SurfaceAppearance` map set the scene has not uploaded.
    pub(crate) fn patched(&self, resolved: &Resolved) -> Option<ResolvedInstance> {
        let mesh = resolved.meshes.get(&self.mesh)?;
        if self.alpha <= 0.0 {
            return None;
        }
        let texture = self
            .texture
            .clone()
            .filter(|reference| resolved.images.contains_key(reference));
        let appearance = match &self.appearance {
            Some(planned) => {
                let wanted = planned.resolved(&resolved.images);
                Some(
                    resolved
                        .appearances
                        .iter()
                        .position(|known| *known == wanted)?,
                )
            }
            None => None,
        };

        Some(ResolvedInstance {
            referent: self.referent,
            mesh: self.mesh.clone(),
            material: self.material,
            texture,
            appearance,
            model: self.fit.transform(mesh),
            color: self.color,
            alpha: self.alpha,
            reflectance: self.reflectance,
            casts_shadow: self.casts_shadow,
        })
    }
}
