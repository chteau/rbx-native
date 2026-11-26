//! A `ViewportFrame`'s `MeshPart`s and file `SpecialMesh`es (an
//! `EditableMesh` behind one included): planned with the frame, resolved
//! against the very meshes and images the workspace's are, once those are
//! in. Until then, and for good where one never arrives, the part's box
//! stands in, as it does in the workspace.

use std::collections::HashMap;
use std::sync::Arc;

use rbx_assets::AssetRef;

use crate::assets::Image;
use crate::scene::filemesh::{self, Appearance, Plan};
use crate::scene::{Part, Resolved, ResolvedInstance};

#[derive(Clone, Default)]
pub(crate) struct ViewportMeshes {
    plan: Arc<Plan>,
    pub(crate) instances: Vec<ResolvedInstance>,
    meshes: HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
    images: HashMap<AssetRef, Arc<Image>>,
    appearances: Vec<Appearance>,
}

impl ViewportMeshes {
    pub(super) fn new(plan: Plan) -> Self {
        ViewportMeshes {
            plan: Arc::new(plan),
            ..ViewportMeshes::default()
        }
    }

    /// The mesh and texture assets the frame wants.
    pub(crate) fn assets(&self, meshes: &mut Vec<AssetRef>, textures: &mut Vec<AssetRef>) {
        meshes.extend(self.plan.mesh_refs());
        textures.extend(self.plan.texture_refs());
    }

    /// Joins the plan to whichever of `meshes` and `images` it names, and
    /// hides the box of every part that got real geometry. Safe to call
    /// again over more of them.
    pub(crate) fn resolve(
        &mut self,
        parts: &mut [Part],
        meshes: &HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>>,
        images: &HashMap<AssetRef, Arc<Image>>,
    ) {
        let (mut wanted_meshes, mut wanted_images) = (Vec::new(), Vec::new());
        self.assets(&mut wanted_meshes, &mut wanted_images);
        let (resolved, hidden) = filemesh::resolve(
            &self.plan,
            pick(&wanted_meshes, meshes),
            pick(&wanted_images, images),
        );
        for part in parts.iter_mut() {
            if hidden.contains(&part.referent()) {
                part.suppressed = true;
            }
        }
        self.instances = resolved.instances;
        self.meshes = resolved.meshes;
        self.images = resolved.images;
        self.appearances = resolved.appearances;
    }

    /// What the renderer draws the frame's meshes from.
    pub(crate) fn resolved(&self) -> Resolved {
        let mut resolved = Resolved::default();
        resolved.meshes.clone_from(&self.meshes);
        resolved.images.clone_from(&self.images);
        resolved.appearances.clone_from(&self.appearances);
        for instance in &self.instances {
            resolved.push(instance.clone());
        }
        resolved
    }
}

/// Two frames draw the same meshes when they resolved the same instances
/// in the same places, which is all a layout compares them for.
impl PartialEq for ViewportMeshes {
    fn eq(&self, other: &Self) -> bool {
        let key = |i: &ResolvedInstance| (i.referent, i.mesh.clone(), i.texture.clone(), i.model, i.color, i.alpha);
        Arc::ptr_eq(&self.plan, &other.plan)
            && self.instances.len() == other.instances.len()
            && self
                .instances
                .iter()
                .zip(&other.instances)
                .all(|(a, b)| key(a) == key(b))
    }
}

impl std::fmt::Debug for ViewportMeshes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportMeshes")
            .field("instances", &self.instances.len())
            .finish()
    }
}

/// The entries of `from` that `wanted` names.
fn pick<T>(wanted: &[AssetRef], from: &HashMap<AssetRef, Arc<T>>) -> HashMap<AssetRef, Arc<T>> {
    wanted
        .iter()
        .filter_map(|reference| Some((reference.clone(), Arc::clone(from.get(reference)?))))
        .collect()
}
