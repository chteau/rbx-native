//! Unit tests for [`super`]: which parts an isolation hides, and that the
//! answer holds across edits, landings and the isolation's end.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::pick::{parts_along, Meshes, Ray};
use crate::scene::Drawn;
use crate::scene::UnionEvaluations;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const WORKSPACE: u32 = 100;
const RIG: u32 = 101;
const TORSO: u32 = 1;
const HEAD: u32 = 2;
const WALL: u32 = 3;
const STATUE: u32 = 4;

fn part(referent: u32, class: &str, x: f32) -> Instance {
    let mut instance = Instance::new(Ref::new(referent), class, class);
    let properties = instance.properties_mut();
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    );
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y: 0.0, z: 0.0 },
            rotation: IDENTITY_ROTATION,
        }),
    );
    instance
}

fn mesh_part(referent: u32, x: f32) -> Instance {
    let mut instance = part(referent, "MeshPart", x);
    instance.properties_mut().insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    instance
}

fn cube() -> rbx_mesh::SkinnedMesh {
    rbx_mesh::SkinnedMesh::from(rbx_mesh::Mesh {
        version: (4, 1),
        vertices: Vec::new(),
        indices: Vec::new(),
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-0.5; 3],
            max: [0.5; 3],
        },
    })
}

fn meshes() -> HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>> {
    HashMap::from([(AssetRef::Id(1), Arc::new(cube()))])
}

/// `Workspace { Rig { Torso, Head (a mesh) }, Wall, Statue (a mesh) }`, its
/// meshes resolved.
fn place() -> (WeakDom, Scene) {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(WORKSPACE), "Workspace", "Workspace"));
    dom.insert(Instance::new(Ref::new(RIG), "Model", "Rig"));
    dom.insert(part(TORSO, "Part", 0.0));
    dom.insert(mesh_part(HEAD, 0.0));
    dom.insert(part(WALL, "Part", 10.0));
    dom.insert(mesh_part(STATUE, -10.0));
    dom.set_parent(Ref::new(WORKSPACE), None);
    dom.set_parent(Ref::new(RIG), Some(Ref::new(WORKSPACE)));
    for (child, parent) in [
        (TORSO, RIG),
        (HEAD, RIG),
        (WALL, WORKSPACE),
        (STATUE, WORKSPACE),
    ] {
        dom.set_parent(Ref::new(child), Some(Ref::new(parent)));
    }
    let mut scene = Scene::from_dom(&dom, &ReflectionDatabase::embedded()).unwrap();
    scene.resolve_file_meshes(meshes(), HashMap::new());
    (dom, scene)
}

fn isolate_rig(scene: &mut Scene, dom: &WeakDom) {
    scene.isolate(Some(&[Ref::new(RIG)]), |at| dom.parent(at));
}

fn whole(scene: &Scene, referent: u32) -> Part {
    *scene
        .parts()
        .iter()
        .find(|part| part.id.is_whole() && part.referent() == Ref::new(referent))
        .expect("the part is in the scene")
}

fn instance_drawn(scene: &Scene, referent: u32) -> bool {
    scene
        .resolved_file_meshes()
        .instances
        .iter()
        .find(|instance| instance.referent == Ref::new(referent))
        .expect("the mesh resolved")
        .is_drawn()
}

fn resync(scene: &mut Scene, dom: &WeakDom, referent: u32) -> Drawn {
    let known = scene.materials().layers();
    scene
        .resync_part(
            dom,
            &ReflectionDatabase::embedded(),
            Ref::new(referent),
            known,
            &UnionEvaluations::default(),
        )
        .expect("a patch, not a rebuild")
        .drawn
}

#[test]
fn a_root_and_everything_under_it_is_within_and_nothing_else_is() {
    let (dom, _) = place();
    let roots = HashSet::from([Ref::new(RIG)]);
    let parent = |at| dom.parent(at);

    assert!(within(&roots, Ref::new(RIG), parent), "the root itself");
    assert!(within(&roots, Ref::new(TORSO), parent));
    assert!(within(&roots, Ref::new(HEAD), parent));
    assert!(!within(&roots, Ref::new(WALL), parent), "a sibling");
    assert!(!within(&roots, Ref::new(WORKSPACE), parent), "an ancestor");
    assert!(!within(&HashSet::new(), Ref::new(TORSO), parent));
}

#[test]
fn only_what_stands_under_the_roots_is_drawn() {
    let (dom, mut scene) = place();
    isolate_rig(&mut scene, &dom);

    assert!(whole(&scene, TORSO).is_drawn());
    assert!(whole(&scene, TORSO).casts_shadow());
    assert!(!whole(&scene, WALL).is_drawn());
    assert!(
        !whole(&scene, WALL).casts_shadow(),
        "no shadow onto the rig"
    );
    assert!(instance_drawn(&scene, HEAD));
    assert!(!instance_drawn(&scene, STATUE));

    // Decals, emitters and canvases stand on these: nothing for the wall.
    let placements = scene.placements();
    assert!(placements.contains_key(&Ref::new(TORSO)));
    assert!(!placements.contains_key(&Ref::new(WALL)));
    assert!(scene.placement_of(Ref::new(WALL)).is_none());
    assert!(scene.placement_of_any(Ref::new(WALL)).is_some());
    // The outline and the gizmo still do: the wall keeps its `Ref`.
    assert!(scene.all_placements().contains_key(&Ref::new(WALL)));
}

#[test]
fn a_hidden_part_is_empty_space_to_a_click() {
    let (dom, mut scene) = place();
    let database = ReflectionDatabase::embedded();
    // Straight down the +X axis, through the torso and on into the wall.
    let ray = Ray::new(Vec3::new(-5.0, 0.0, 0.0), Vec3::X);

    let everything = parts_along(&dom, &database, &Meshes::of(&scene), ray);
    assert!(everything.contains(&Ref::new(WALL)));

    isolate_rig(&mut scene, &dom);
    let isolated = Meshes::of(&scene);
    let hits = parts_along(&dom, &database, &isolated, ray);
    assert!(hits.contains(&Ref::new(TORSO)));
    assert!(!hits.contains(&Ref::new(WALL)));
    // A click that only ever met the wall now meets nothing.
    let beside = Ray::new(Vec3::new(10.0, 0.0, -5.0), Vec3::Z);
    assert!(parts_along(&dom, &database, &isolated, beside).is_empty());
    assert!(isolated.shows(&dom, Ref::new(HEAD)));
    assert!(!isolated.shows(&dom, Ref::new(STATUE)));
}

#[test]
fn a_part_added_under_a_root_later_is_drawn_and_one_outside_is_not() {
    let (mut dom, mut scene) = place();
    isolate_rig(&mut scene, &dom);

    dom.insert(part(5, "Part", 2.0));
    dom.set_parent(Ref::new(5), Some(Ref::new(RIG)));
    let Drawn::Box(arm) = resync(&mut scene, &dom, 5) else {
        panic!("a plain part draws as its box");
    };
    assert!(arm.is_drawn());
    assert!(whole(&scene, 5).is_drawn());

    dom.insert(part(6, "Part", 20.0));
    dom.set_parent(Ref::new(6), Some(Ref::new(WORKSPACE)));
    let Drawn::Box(crate_) = resync(&mut scene, &dom, 6) else {
        panic!("a plain part draws as its box");
    };
    assert!(!crate_.is_drawn());
}

#[test]
fn moving_a_part_out_from_under_a_root_hides_it_and_back_shows_it() {
    let (mut dom, mut scene) = place();
    isolate_rig(&mut scene, &dom);

    dom.set_parent(Ref::new(TORSO), Some(Ref::new(WORKSPACE)));
    resync(&mut scene, &dom, TORSO);
    assert!(!whole(&scene, TORSO).is_drawn());

    dom.set_parent(Ref::new(TORSO), Some(Ref::new(RIG)));
    resync(&mut scene, &dom, TORSO);
    assert!(whole(&scene, TORSO).is_drawn());

    // A mesh part carries the answer onto the instance it draws through.
    dom.set_parent(Ref::new(STATUE), Some(Ref::new(RIG)));
    assert!(matches!(
        resync(&mut scene, &dom, STATUE),
        Drawn::Mesh { .. }
    ));
    assert!(instance_drawn(&scene, STATUE));
}

// What the Animation Editor does sixty times a second: a rig part's CFrame
// written through the ordinary change path, never a rebuild.
#[test]
fn posing_a_rig_part_keeps_it_drawn_and_the_rest_hidden() {
    let (mut dom, mut scene) = place();
    isolate_rig(&mut scene, &dom);

    for step in 0..3 {
        let _ = dom.set_property(
            Ref::new(TORSO),
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: step as f32,
                    z: 0.0,
                },
                rotation: IDENTITY_ROTATION,
            }),
        );
        let Drawn::Box(torso) = resync(&mut scene, &dom, TORSO) else {
            panic!("a plain part draws as its box");
        };
        assert!(torso.is_drawn());
        assert!(!whole(&scene, WALL).is_drawn());
    }
}

#[test]
fn a_mesh_landing_while_isolated_stays_isolated() {
    let (dom, mut scene) = place();
    isolate_rig(&mut scene, &dom);

    // A streaming tick rebuilds the resolved set from the plan, with no DOM.
    scene.resolve_file_meshes(meshes(), HashMap::new());

    assert!(instance_drawn(&scene, HEAD));
    assert!(!instance_drawn(&scene, STATUE));
}

#[test]
fn lifting_the_isolation_restores_everything() {
    let (dom, mut scene) = place();
    let before: Vec<Part> = scene.parts().to_vec();
    let placements = scene.placements();

    isolate_rig(&mut scene, &dom);
    assert!(scene.isolation().is_some());
    scene.isolate(None, |at| dom.parent(at));

    assert!(scene.isolation().is_none());
    assert_eq!(scene.parts(), &before[..]);
    assert_eq!(scene.placements(), placements);
    assert!(instance_drawn(&scene, HEAD) && instance_drawn(&scene, STATUE));
    assert!(Meshes::of(&scene).shows(&dom, Ref::new(WALL)));
}
