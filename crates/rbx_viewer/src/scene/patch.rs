//! `Scene::patch_part`'s counterpart for a part whose box a real mesh has
//! replaced — see [`Scene::patch_mesh_instance`].

use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{filemesh, union, ResolvedInstance, Scene};

impl Scene {
    /// [`Scene::patch_part`] for a `MeshPart`, a `Part` wearing a `SpecialMesh`,
    /// or a union whose real geometry resolved: those parts' boxes are
    /// suppressed, so `patch_part` refuses them, and what actually draws is a
    /// [`ResolvedInstance`] in `self.resolved_file_meshes` — this recomputes
    /// that one instance from `dom` instead, against the meshes and images
    /// already downloaded.
    ///
    /// Same contract as `patch_part`: `Some(index)` (into
    /// [`Scene::resolved_file_meshes`]`().instances`) means the patched
    /// instance is already in place for the caller to upload; `None` means
    /// only a full reload draws the right picture — the edit crossed a batch
    /// (see [`batch`]), turned the part invisible, would need a
    /// `SurfaceAppearance` map set or a material layer (past
    /// `known_material_layers`) that was never uploaded, or repainted a union
    /// from its operation tree (see `union::Entry::patched`).
    pub(crate) fn patch_mesh_instance(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        known_material_layers: usize,
    ) -> Option<usize> {
        let index = self
            .resolved_file_meshes
            .instances
            .iter()
            .position(|instance| instance.referent == referent)?;

        let patched = filemesh::replan(dom, database, referent, &mut self.materials)
            .and_then(|entry| entry.patched(&self.resolved_file_meshes))
            .or_else(|| {
                union::replan(dom, database, referent, &mut self.materials)
                    .and_then(|entry| entry.patched())
            })?;

        let before = &self.resolved_file_meshes.instances[index];
        if patched.material.layer as usize >= known_material_layers
            || batch(&patched) != batch(before)
        {
            return None;
        }

        self.resolved_file_meshes.instances[index] = patched;
        Some(index)
    }
}

/// What a resolved mesh's edit is not allowed to change without a full reload
/// — the same idea as `scene::bucket` for a boxed part: the (mesh, texture,
/// appearance) triple is one colour-pass batch, `alpha < 1` picks that batch's
/// opaque or blended copy (see `renderer::filemesh`), and `casts_shadow`
/// whether the shadow pass holds an instance to rewrite at all (see
/// `renderer::shadow::casters::mesh_batches`).
fn batch(instance: &ResolvedInstance) -> (&AssetRef, Option<&AssetRef>, Option<usize>, bool, bool) {
    (
        &instance.mesh,
        instance.texture.as_ref(),
        instance.appearance,
        instance.alpha < 1.0,
        instance.casts_shadow,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use glam::Vec3;
    use rbx_assets::AssetRef;
    use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use crate::scene::{srgb_to_linear, Scene};

    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    fn mesh_part() -> Ref {
        Ref::new(1)
    }

    fn plain_part() -> Ref {
        Ref::new(2)
    }

    fn cframe_at(x: f32, y: f32, z: f32) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY_ROTATION,
        })
    }

    fn fake_mesh() -> rbx_mesh::Mesh {
        rbx_mesh::Mesh {
            version: (4, 1),
            vertices: Vec::new(),
            indices: Vec::new(),
            lods: Vec::new(),
            bounds: rbx_mesh::Aabb {
                min: [-0.5; 3],
                max: [0.5; 3],
            },
        }
    }

    /// A `MeshPart` whose mesh "downloaded", next to a plain `Part` that
    /// still draws as a box.
    fn resolved_place() -> (WeakDom, Scene) {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(100);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.set_parent(workspace, None);
        for (referent, class) in [(mesh_part(), "MeshPart"), (plain_part(), "Part")] {
            let mut instance = Instance::new(referent, class, class);
            let properties = instance.properties_mut();
            properties.insert(
                "MeshId".to_string(),
                Variant::String("rbxassetid://1".to_string()),
            );
            properties.insert(
                "size".to_string(),
                Variant::Vector3(Vector3Data {
                    x: 2.0,
                    y: 2.0,
                    z: 2.0,
                }),
            );
            properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
            properties.insert(
                "Color3uint8".to_string(),
                Variant::Color3uint8 { r: 0, g: 0, b: 255 },
            );
            dom.insert(instance);
            dom.set_parent(referent, Some(workspace));
        }

        let database = ReflectionDatabase::embedded();
        let mut scene = Scene::from_dom(&dom, &database).unwrap();
        let mut meshes = HashMap::new();
        meshes.insert(AssetRef::Id(1), fake_mesh());
        scene.resolve_file_meshes(meshes, HashMap::new());
        assert_eq!(scene.resolved_file_meshes().instances.len(), 1);
        (dom, scene)
    }

    // The case `Scene::patch_part` cannot serve: the MeshPart's box is
    // suppressed, so only the resolved instance can carry a colour edit.
    #[test]
    fn patch_mesh_instance_updates_a_colour_edit_in_place() {
        let (mut dom, mut scene) = resolved_place();
        let database = ReflectionDatabase::embedded();
        let known = scene.materials().layers();
        assert!(scene
            .patch_part(&dom, &database, mesh_part(), known)
            .is_none());

        dom.set_property(
            mesh_part(),
            "Color3uint8",
            Variant::Color3uint8 { r: 255, g: 0, b: 0 },
        )
        .unwrap();
        let index = scene
            .patch_mesh_instance(&dom, &database, mesh_part(), known)
            .expect("a colour-only edit must patch in place");

        let instance = &scene.resolved_file_meshes().instances[index];
        assert_eq!(instance.referent, mesh_part());
        assert_eq!(instance.color, [srgb_to_linear(1.0), 0.0, 0.0]);
    }

    #[test]
    fn patch_mesh_instance_follows_a_cframe_edit() {
        let (mut dom, mut scene) = resolved_place();
        let database = ReflectionDatabase::embedded();
        let known = scene.materials().layers();

        dom.set_property(mesh_part(), "CFrame", cframe_at(5.0, 6.0, 7.0))
            .unwrap();
        let index = scene
            .patch_mesh_instance(&dom, &database, mesh_part(), known)
            .unwrap();

        let model = scene.resolved_file_meshes().instances[index].model;
        assert!(model
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::new(5.0, 6.0, 7.0), 1e-5));
    }

    // Crossing into the blended pass moves the instance to a different
    // `renderer::filemesh` batch — not a slot the caller can write into.
    #[test]
    fn patch_mesh_instance_falls_back_when_transparency_crosses_into_blended() {
        let (mut dom, mut scene) = resolved_place();
        let database = ReflectionDatabase::embedded();
        let known = scene.materials().layers();

        dom.set_property(mesh_part(), "Transparency", Variant::Float32(0.5))
            .unwrap();

        assert!(scene
            .patch_mesh_instance(&dom, &database, mesh_part(), known)
            .is_none());
    }

    // The shadow pass only holds casters, so opting out has no slot left to
    // rewrite — and opting back in has none to write into.
    #[test]
    fn patch_mesh_instance_falls_back_when_cast_shadow_flips() {
        let (mut dom, mut scene) = resolved_place();
        let database = ReflectionDatabase::embedded();
        let known = scene.materials().layers();

        dom.set_property(mesh_part(), "CastShadow", Variant::Bool(false))
            .unwrap();

        assert!(scene
            .patch_mesh_instance(&dom, &database, mesh_part(), known)
            .is_none());
    }

    // A part still drawn as its box is `patch_part`'s business; this path
    // must never claim it.
    #[test]
    fn patch_mesh_instance_falls_back_for_a_part_drawn_as_a_box() {
        let (dom, mut scene) = resolved_place();
        let database = ReflectionDatabase::embedded();
        let known = scene.materials().layers();

        assert!(scene
            .patch_mesh_instance(&dom, &database, plain_part(), known)
            .is_none());
        assert!(scene
            .patch_part(&dom, &database, plain_part(), known)
            .is_some());
    }
}
