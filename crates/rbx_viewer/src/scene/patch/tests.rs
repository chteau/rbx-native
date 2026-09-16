//! Unit tests for [`super`]: which edits to a resolved mesh stay a
//! single-instance patch, which fall back to the box while their asset is
//! fetched, and which still need a full reload.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{srgb_to_linear, MeshPatch, Scene};

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
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh()));
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
    let index = placed(&mut scene, &dom, mesh_part());

    let instance = &scene.resolved_file_meshes().instances[index];
    assert_eq!(instance.referent, mesh_part());
    assert_eq!(instance.color, [srgb_to_linear(1.0), 0.0, 0.0]);
}

#[test]
fn patch_mesh_instance_follows_a_cframe_edit() {
    let (mut dom, mut scene) = resolved_place();

    dom.set_property(mesh_part(), "CFrame", cframe_at(5.0, 6.0, 7.0))
        .unwrap();
    let index = placed(&mut scene, &dom, mesh_part());

    let model = scene.resolved_file_meshes().instances[index].model;
    assert!(model
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(5.0, 6.0, 7.0), 1e-5));
}

/// The index `patch_mesh_instance` placed `referent` at, asserting it did.
fn placed(scene: &mut Scene, dom: &WeakDom, referent: Ref) -> usize {
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();
    match scene.patch_mesh_instance(dom, &database, referent, known) {
        Some(MeshPatch::Placed(index)) => index,
        other => panic!("expected the instance to be placed, got {other:?}"),
    }
}

// Crossing into the blended pass moves the instance to a different
// `renderer::filemesh` batch, which is the renderer's business now — the
// scene still has one instance to hand it.
#[test]
fn patch_mesh_instance_keeps_a_transparency_crossing_in_place() {
    let (mut dom, mut scene) = resolved_place();

    dom.set_property(mesh_part(), "Transparency", Variant::Float32(0.5))
        .unwrap();
    let index = placed(&mut scene, &dom, mesh_part());

    let instance = &scene.resolved_file_meshes().instances[index];
    assert_eq!(instance.referent, mesh_part());
    assert!((instance.alpha - 0.5).abs() < 1e-6);
    assert_eq!(scene.resolved_file_meshes().instances.len(), 1);
}

// Opting out of the shadow pass (and back in) only flips the flag the
// renderer's caster sync reads.
#[test]
fn patch_mesh_instance_keeps_a_cast_shadow_flip_in_place() {
    let (mut dom, mut scene) = resolved_place();

    dom.set_property(mesh_part(), "CastShadow", Variant::Bool(false))
        .unwrap();
    let index = placed(&mut scene, &dom, mesh_part());
    assert!(!scene.resolved_file_meshes().instances[index].casts_shadow);

    dom.set_property(mesh_part(), "CastShadow", Variant::Bool(true))
        .unwrap();
    let index = placed(&mut scene, &dom, mesh_part());
    assert!(scene.resolved_file_meshes().instances[index].casts_shadow);
}

// A fully transparent mesh is dropped from the resolved set (a full build
// never lists one) and reported as such, and reappears once visible again —
// its suppressed box is what says the referent still belongs to this path.
#[test]
fn patch_mesh_instance_removes_an_invisible_mesh_and_restores_it() {
    let (mut dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();

    dom.set_property(mesh_part(), "Transparency", Variant::Float32(1.0))
        .unwrap();
    assert_eq!(
        scene.patch_mesh_instance(&dom, &database, mesh_part(), known),
        Some(MeshPatch::Removed)
    );
    assert!(scene.resolved_file_meshes().instances.is_empty());
    assert!(scene
        .patch_part(&dom, &database, mesh_part(), known)
        .is_none());

    dom.set_property(mesh_part(), "Transparency", Variant::Float32(0.0))
        .unwrap();
    let index = placed(&mut scene, &dom, mesh_part());
    assert_eq!(scene.resolved_file_meshes().instances.len(), 1);
    assert_eq!(scene.resolved_file_meshes().instances[index].alpha, 1.0);
}

// Swapping to a mesh that is already in the resolved set is a batch move;
// swapping to one nobody has decoded is refused here, and the caller draws
// the fallback box instead (see `fall_back_to_box` below).
#[test]
fn patch_mesh_instance_follows_a_mesh_id_swap_only_to_a_downloaded_mesh() {
    let (mut dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();

    dom.set_property(
        mesh_part(),
        "MeshId",
        Variant::String("rbxassetid://2".to_string()),
    )
    .unwrap();
    assert!(scene
        .patch_mesh_instance(&dom, &database, mesh_part(), known)
        .is_none());

    scene
        .resolved_file_meshes
        .meshes
        .insert(AssetRef::Id(2), std::sync::Arc::new(fake_mesh()));
    let index = placed(&mut scene, &dom, mesh_part());
    assert_eq!(
        scene.resolved_file_meshes().instances[index].mesh,
        AssetRef::Id(2)
    );
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

// The decision a landing is judged by, from the other end: what one
// referent would need drawn as the DOM now describes it.
#[test]
fn replan_assets_of_reports_the_mesh_and_texture_an_edit_now_names() {
    let (mut dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();

    dom.set_property(
        mesh_part(),
        "MeshId",
        Variant::String("rbxassetid://2".to_string()),
    )
    .unwrap();
    dom.set_property(
        mesh_part(),
        "TextureID",
        Variant::String("rbxassetid://3".to_string()),
    )
    .unwrap();

    let (meshes, images) = scene.replan_assets_of(&dom, &database, mesh_part());

    assert_eq!(meshes, vec![AssetRef::Id(2)]);
    assert!(images.contains(&AssetRef::Id(3)));
}

// The plan, not just the report: a landing is joined to the place through
// the plan, with no DOM left to re-read, so an edit has to leave it saying
// what the instance draws now.
#[test]
fn replan_assets_of_leaves_the_plan_naming_the_new_mesh() {
    let (mut dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();
    dom.set_property(
        mesh_part(),
        "MeshId",
        Variant::String("rbxassetid://2".to_string()),
    )
    .unwrap();
    scene.replan_assets_of(&dom, &database, mesh_part());

    // What a later landing would resolve against, with both meshes in.
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh()));
    meshes.insert(AssetRef::Id(2), Arc::new(fake_mesh()));
    scene.resolve_file_meshes(meshes, HashMap::new());

    let instances = &scene.resolved_file_meshes().instances;
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].mesh, AssetRef::Id(2));
}

// An edit naming geometry nobody has fetched draws the box the `MeshPart`
// already falls back to, instead of a reload: the part stops being
// suppressed and its resolved instance goes.
#[test]
fn fall_back_to_box_returns_an_unsuppressed_part_and_drops_the_instance() {
    let (mut dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();
    dom.set_property(
        mesh_part(),
        "MeshId",
        Variant::String("rbxassetid://2".to_string()),
    )
    .unwrap();
    // The path the caller takes first, and the one that refuses.
    assert!(scene
        .patch_mesh_instance(&dom, &database, mesh_part(), known)
        .is_none());

    let index = scene
        .fall_back_to_box(&dom, &database, mesh_part(), known)
        .expect("a MeshPart always has a box to fall back to");

    let part = scene.parts()[index];
    assert_eq!(part.referent, mesh_part());
    assert!(!part.is_suppressed(), "the box draws again");
    assert!(scene.resolved_file_meshes().instances.is_empty());
}

// And only for a referent that has one: a part this scene never built is
// still the caller's cue to reload.
#[test]
fn fall_back_to_box_refuses_a_referent_the_scene_never_built() {
    let (dom, mut scene) = resolved_place();
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();

    assert!(scene
        .fall_back_to_box(&dom, &database, Ref::new(999), known)
        .is_none());
    // A part still drawn as its box is `patch_part`'s, not this path's.
    assert!(scene
        .fall_back_to_box(&dom, &database, plain_part(), known)
        .is_none());
}
