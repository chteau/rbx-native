//! `Scene::patch_part`'s counterpart for a part whose box a real mesh has
//! replaced — see [`Scene::patch_mesh_instance`] — and the two things a
//! single-instance edit that names an asset nobody has fetched yet needs:
//! what to fetch ([`Scene::replan_assets_of`]) and what to draw meanwhile
//! ([`Scene::fall_back_to_box`]).

use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use glam::Vec3;

use super::material::Catalog;
use super::{filemesh, shape, union, Resolved, ResolvedInstance, Scene};

/// What [`Scene::patch_mesh_instance`] did to the resolved set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshPatch {
    /// `instances[index]` holds the patched instance — rewritten where it
    /// was, or appended if the part was invisible until this edit.
    Placed(usize),
    /// The part turned fully transparent: its instance is gone from the
    /// set, the way a full build never lists one, and the renderer has to
    /// drop its copies too.
    Removed,
}

/// One re-planned entry of either kind, so the two can be asked the same
/// questions — see [`Scene::patch_mesh_instance`].
enum Replanned {
    Mesh(filemesh::Entry),
    Union(union::Entry),
}

impl Replanned {
    /// The entry `referent` would get from a fresh `filemesh::plan` or
    /// `union::plan`, or `None` when it is neither any more (a `MeshPart`
    /// whose `MeshId` was just emptied, say).
    fn of(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        materials: &mut Catalog,
    ) -> Option<Self> {
        filemesh::replan(dom, database, referent, materials)
            .map(Replanned::Mesh)
            .or_else(|| union::replan(dom, database, referent, materials).map(Replanned::Union))
    }

    /// `Transparency` 1: a fresh resolution would drop the instance outright.
    fn is_invisible(&self) -> bool {
        match self {
            Replanned::Mesh(entry) => entry.is_invisible(),
            Replanned::Union(entry) => entry.is_invisible(),
        }
    }

    /// The instance a fresh resolution would build against what already
    /// downloaded — `None` where it would need something only a full reload
    /// fetches or computes (see `filemesh::Entry::patched` and
    /// `union::Entry::patched`).
    fn patched(&self, resolved: &Resolved) -> Option<ResolvedInstance> {
        match self {
            Replanned::Mesh(entry) => entry.patched(resolved),
            Replanned::Union(entry) => entry.patched(),
        }
    }
}

impl Scene {
    /// [`Scene::patch_part`] for a `MeshPart`, a `Part` wearing a `SpecialMesh`,
    /// or a union whose real geometry resolved: those parts' boxes are
    /// suppressed, so `patch_part` refuses them, and what actually draws is a
    /// [`ResolvedInstance`] in `self.resolved_file_meshes` — this recomputes
    /// that one instance from `dom` instead, against the meshes and images
    /// already downloaded.
    ///
    /// Same contract as `patch_part`: `Some` means the resolved set is
    /// already updated for the caller to hand on (see [`MeshPatch`] for the
    /// two ways it can be), whatever batch the renderer has to move the
    /// instance to — a `Transparency` that crossed 0, a `CastShadow` toggle,
    /// even a `MeshId`/`TextureID` swap to an asset another instance already
    /// draws through. `None` means only a full reload draws the right
    /// picture: the part's box was never suppressed (it is `patch_part`'s),
    /// the edit needs a mesh, texture, `SurfaceAppearance` map set or
    /// material layer (past `known_material_layers`) that was never
    /// downloaded, or it repainted a union from its operation tree (see
    /// `union::Entry::patched`).
    pub(crate) fn patch_mesh_instance(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        known_material_layers: usize,
    ) -> Option<MeshPatch> {
        // The suppressed box, not the instance, is what says this referent
        // belongs here: an invisible mesh has no instance at all, and has
        // to be able to come back once visible again.
        if !self
            .parts
            .iter()
            .any(|part| part.referent == referent && part.is_suppressed())
        {
            return None;
        }
        let instances = &mut self.resolved_file_meshes.instances;
        let existing = instances
            .iter()
            .position(|instance| instance.referent == referent);

        let entry = Replanned::of(dom, database, referent, &mut self.materials)?;
        if entry.is_invisible() {
            if let Some(index) = existing {
                // Order only matters to a full build's batch construction;
                // the renderer finds instances by referent from here on.
                instances.swap_remove(index);
            }
            return Some(MeshPatch::Removed);
        }
        let patched = entry.patched(&self.resolved_file_meshes)?;
        if patched.material.layer as usize >= known_material_layers {
            return None;
        }

        let instances = &mut self.resolved_file_meshes.instances;
        let index = match existing {
            Some(index) => {
                instances[index] = patched;
                index
            }
            None => {
                instances.push(patched);
                instances.len() - 1
            }
        };
        Some(MeshPatch::Placed(index))
    }

    /// Re-plans `referent` against the DOM as it now stands and reports every
    /// asset it would need drawn that way: its mesh, and the images its own
    /// `TextureID`, its `SurfaceAppearance` and its material sample.
    ///
    /// Installing the fresh plan entry matters as much as the report does.
    /// The file mesh plan is what a later landing joins the place from — there
    /// is no DOM left to re-plan from by then — so an edit that changed what
    /// this instance draws has to leave it saying so, or the mesh that lands
    /// would be the `MeshId` the file was opened with.
    pub(crate) fn replan_assets_of(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
    ) -> (Vec<AssetRef>, Vec<AssetRef>) {
        let entry = filemesh::replan(dom, database, referent, &mut self.materials);
        let (meshes, mut images) = match &entry {
            Some(entry) => {
                let (mesh, images) = entry.assets();
                (vec![mesh], images)
            }
            None => (Vec::new(), Vec::new()),
        };
        self.file_mesh_plan.install(referent, entry);

        if let Some(instance) = dom.get(referent) {
            let slot = self.materials.slot_for(instance.properties(), database);
            images.extend(self.materials.maps_of(slot.layer));
        }
        (meshes, images)
    }

    /// Puts `referent` back on the box a `MeshPart` falls back to, for an edit
    /// naming geometry this session has never decoded.
    ///
    /// The box is what Roblox itself shows while a mesh streams in, and it is
    /// what this viewer already draws for a `MeshId` that never resolved — so
    /// there is nothing new to invent for the moments between the edit and the
    /// asset landing. `Some(index)` means `self.parts[index]` holds it and the
    /// resolved instance is gone; the caller hands both on to the renderer.
    ///
    /// `None` where no box stands for the referent: a union, whose recovered
    /// pieces all answer to the union's own referent (see `union::tree`); one
    /// this scene never built; or one whose edit also named a material layer
    /// past `known_material_layers`, which is a texture array only a rebuild
    /// resizes.
    pub(crate) fn fall_back_to_box(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        known_material_layers: usize,
    ) -> Option<usize> {
        if union::replan(dom, database, referent, &mut self.materials).is_some() {
            return None;
        }
        let index = self
            .parts
            .iter()
            .position(|part| part.referent == referent && part.is_suppressed())?;

        let instance = dom.get(referent)?;
        let properties = instance.properties();
        let Some(Variant::Vector3(size)) = properties.get("size") else {
            return None;
        };
        let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
            return None;
        };
        let size = Vec3::new(size.x, size.y, size.z);
        let geometry = shape::resolve(dom, database, instance, size);
        let patched = super::assemble_part(
            properties,
            database,
            &mut self.materials,
            geometry,
            super::cframe_matrix(cframe),
            referent,
        );
        if patched.material.layer as usize >= known_material_layers {
            return None;
        }

        // `assemble_part` builds an unsuppressed part, which is the whole
        // point: the box draws again until the mesh lands.
        self.parts[index] = patched;
        self.resolved_file_meshes
            .instances
            .retain(|instance| instance.referent != referent);
        Some(index)
    }
}

#[cfg(test)]
#[path = "patch/tests.rs"]
mod tests;
