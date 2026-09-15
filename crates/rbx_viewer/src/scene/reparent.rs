//! Whether a `Parent` change left a scene with nothing new to draw — see
//! [`Scene::already_draws`].

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{descendants_of, is_drawable, workspace_descendants, Scene};

impl Scene {
    /// Whether `referent`, just reparented in `dom`, still sits under
    /// `Workspace` with every drawable part in its subtree already built by
    /// this scene — in which case the move changed nothing visible: a part's
    /// `CFrame` is world-space, and whatever hangs off it (decals, lights,
    /// emitters, attachments) moved with it and still hangs off it.
    ///
    /// `false` the moment either half fails. A subtree that just left
    /// `Workspace` has to stop drawing; one that just entered it has parts
    /// (and decals, meshes, textures) nothing here ever built; and a part
    /// the scene never built — no `size`/`CFrame`, say — is cheaper to
    /// rebuild than to reason about. Only a full reload works those out.
    pub(crate) fn already_draws(
        &self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
    ) -> bool {
        workspace_descendants(dom, database).any(|inside| inside == referent)
            && descendants_of(dom, referent)
                .filter(|&candidate| is_drawable(dom, database, candidate))
                .all(|candidate| self.knows(candidate))
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use crate::scene::Scene;

    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    const WORKSPACE: u32 = 1;
    const STORAGE: u32 = 2;
    const MODEL_A: u32 = 3;
    const MODEL_B: u32 = 4;
    const PART: u32 = 5;
    const STAGED: u32 = 6;

    fn part(referent: u32) -> Instance {
        let mut instance = Instance::new(Ref::new(referent), "Part", "Part");
        let properties = instance.properties_mut();
        properties.insert(
            "size".to_string(),
            Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 1.0,
                z: 2.0,
            }),
        );
        properties.insert(
            "CFrame".to_string(),
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: IDENTITY_ROTATION,
            }),
        );
        instance
    }

    /// `Workspace { ModelA { Part }, ModelB }` next to
    /// `ServerStorage { Part(STAGED) }`.
    fn place() -> (WeakDom, Scene) {
        let mut dom = WeakDom::new();
        for (referent, class) in [
            (WORKSPACE, "Workspace"),
            (STORAGE, "ServerStorage"),
            (MODEL_A, "Model"),
            (MODEL_B, "Model"),
        ] {
            dom.insert(Instance::new(Ref::new(referent), class, class));
        }
        dom.insert(part(PART));
        dom.insert(part(STAGED));
        dom.set_parent(Ref::new(WORKSPACE), None);
        dom.set_parent(Ref::new(STORAGE), None);
        dom.set_parent(Ref::new(MODEL_A), Some(Ref::new(WORKSPACE)));
        dom.set_parent(Ref::new(MODEL_B), Some(Ref::new(WORKSPACE)));
        dom.set_parent(Ref::new(PART), Some(Ref::new(MODEL_A)));
        dom.set_parent(Ref::new(STAGED), Some(Ref::new(STORAGE)));
        let scene = Scene::from_dom(&dom, &ReflectionDatabase::embedded()).unwrap();
        assert_eq!(scene.parts().len(), 1);
        (dom, scene)
    }

    #[test]
    fn a_part_moved_between_two_workspace_models_needs_no_reload() {
        let (mut dom, scene) = place();
        let database = ReflectionDatabase::embedded();

        dom.set_parent(Ref::new(PART), Some(Ref::new(MODEL_B)));

        assert!(scene.already_draws(&dom, &database, Ref::new(PART)));
    }

    #[test]
    fn a_model_moved_under_another_workspace_model_needs_no_reload() {
        let (mut dom, scene) = place();
        let database = ReflectionDatabase::embedded();

        dom.set_parent(Ref::new(MODEL_A), Some(Ref::new(MODEL_B)));

        assert!(scene.already_draws(&dom, &database, Ref::new(MODEL_A)));
    }

    // Leaving `Workspace` means the part has to stop drawing, which no
    // single-instance path does.
    #[test]
    fn a_part_moved_out_of_workspace_needs_a_reload() {
        let (mut dom, scene) = place();
        let database = ReflectionDatabase::embedded();

        dom.set_parent(Ref::new(PART), Some(Ref::new(STORAGE)));

        assert!(!scene.already_draws(&dom, &database, Ref::new(PART)));
    }

    // Entering `Workspace` brings a part the scene never built — and would
    // bring its decals, meshes and textures too.
    #[test]
    fn a_part_moved_into_workspace_needs_a_reload() {
        let (mut dom, scene) = place();
        let database = ReflectionDatabase::embedded();

        dom.set_parent(Ref::new(STAGED), Some(Ref::new(MODEL_B)));

        assert!(!scene.already_draws(&dom, &database, Ref::new(STAGED)));
        assert!(!scene.already_draws(&dom, &database, Ref::new(STORAGE)));
    }
}
