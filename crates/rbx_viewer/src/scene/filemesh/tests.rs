use super::*;
use rbx_dom::{CFrameData, Instance, Vector3Data};

use crate::scene::material::Kind;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// Every entry carries a material slot; these fixtures are about geometry, so
/// they hand the planner a throwaway catalog and ignore what it fills in.
fn planned(dom: &WeakDom) -> Plan {
    let database = database();
    plan(dom, &database, &mut Catalog::new(dom, &database))
}

fn cframe_at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: IDENTITY_ROTATION,
    })
}

fn vector3_variant(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn fake_mesh(bounds_size: [f32; 3]) -> rbx_mesh::Mesh {
    let half: Vec3 = Vec3::from(bounds_size) / 2.0;
    rbx_mesh::Mesh {
        version: (4, 1),
        vertices: Vec::new(),
        indices: Vec::new(),
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: (-half).to_array(),
            max: half.to_array(),
        },
    }
}

#[test]
fn a_mesh_part_reads_mesh_id_as_a_plain_string() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://42".to_string()),
    );
    properties.insert("size".to_string(), vector3_variant(10.0, 10.0, 10.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].mesh, AssetRef::Id(42));
    assert_eq!(plan.mesh_refs(), vec![AssetRef::Id(42)]);
}

#[test]
fn a_mesh_part_reads_mesh_id_wrapped_in_content() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::Content(rbx_dom::Content::Uri("rbxassetid://99".to_string())),
    );
    properties.insert("size".to_string(), vector3_variant(10.0, 10.0, 10.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);

    assert_eq!(plan.entries[0].mesh, AssetRef::Id(99));
}

#[test]
fn mesh_part_scale_fits_size_over_initial_size() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    properties.insert("size".to_string(), vector3_variant(20.0, 40.0, 60.0));
    properties.insert("InitialSize".to_string(), vector3_variant(10.0, 10.0, 10.0));
    properties.insert("CFrame".to_string(), cframe_at(5.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);
    // The mesh's own bounds must be ignored once InitialSize is present.
    let mesh = fake_mesh([2.0, 2.0, 2.0]);
    let transform = plan.entries[0].fit.transform(&mesh);

    assert!(transform
        .transform_vector3(Vec3::X)
        .abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-5));
    assert!(transform
        .transform_vector3(Vec3::Y)
        .abs_diff_eq(Vec3::new(0.0, 4.0, 0.0), 1e-5));
    assert!(transform
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(5.0, 0.0, 0.0), 1e-5));
}

#[test]
fn mesh_part_without_initial_size_fits_the_meshs_own_bounds() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    properties.insert("size".to_string(), vector3_variant(4.0, 4.0, 4.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);
    let mesh = fake_mesh([2.0, 2.0, 2.0]);
    let transform = plan.entries[0].fit.transform(&mesh);

    // Native bounds are 2 studs wide; size asks for 4, so scale is 2x.
    assert!(transform
        .transform_vector3(Vec3::X)
        .abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-5));
}

#[test]
fn a_special_mesh_child_reads_scale_and_offset_ignoring_the_parts_size() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let part_ref = Ref::new(1);
    let mut part = Instance::new(part_ref, "Part", "Part");
    part.properties_mut()
        .insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    part.properties_mut()
        .insert("CFrame".to_string(), cframe_at(10.0, 0.0, 0.0));
    dom.insert(part);
    dom.set_parent(part_ref, Some(workspace));

    let mesh_ref = Ref::new(2);
    let mut mesh_instance = Instance::new(mesh_ref, "SpecialMesh", "Mesh");
    let properties = mesh_instance.properties_mut();
    properties.insert("MeshType".to_string(), Variant::Enum(FILE_MESH));
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://7".to_string()),
    );
    properties.insert("Scale".to_string(), vector3_variant(2.0, 2.0, 2.0));
    properties.insert("Offset".to_string(), vector3_variant(0.0, 1.0, 0.0));
    dom.insert(mesh_instance);
    dom.set_parent(mesh_ref, Some(part_ref));

    let plan = planned(&dom);

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].referent, part_ref);
    let mesh = fake_mesh([100.0, 100.0, 100.0]); // must be ignored entirely
    let transform = plan.entries[0].fit.transform(&mesh);
    assert!(transform
        .transform_vector3(Vec3::X)
        .abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-5));
    assert!(transform
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(10.0, 1.0, 0.0), 1e-5));
}

#[test]
fn a_special_mesh_with_another_mesh_type_is_left_to_shape_resolve() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let part_ref = Ref::new(1);
    let mut part = Instance::new(part_ref, "Part", "Part");
    part.properties_mut()
        .insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    part.properties_mut()
        .insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(part);
    dom.set_parent(part_ref, Some(workspace));

    let mesh_ref = Ref::new(2);
    let mut mesh_instance = Instance::new(mesh_ref, "SpecialMesh", "Mesh");
    // MeshType::Sphere, not FileMesh.
    mesh_instance
        .properties_mut()
        .insert("MeshType".to_string(), Variant::Enum(3));
    dom.insert(mesh_instance);
    dom.set_parent(mesh_ref, Some(part_ref));

    assert!(planned(&dom).entries.is_empty());
}

#[test]
fn an_empty_mesh_id_is_skipped_rather_than_producing_a_dangling_download() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert("MeshId".to_string(), Variant::String(String::new()));
    properties.insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    assert!(planned(&dom).entries.is_empty());
}

#[test]
fn resolve_hides_only_referents_whose_mesh_actually_downloaded() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    for (id, mesh_id) in [(1u32, 1u64), (2u32, 2u64)] {
        let referent = Ref::new(id);
        let mut instance = Instance::new(referent, "MeshPart", "Rock");
        let properties = instance.properties_mut();
        properties.insert(
            "MeshId".to_string(),
            Variant::String(format!("rbxassetid://{mesh_id}")),
        );
        properties.insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
        properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
        dom.insert(instance);
        dom.set_parent(referent, Some(workspace));
    }

    let plan = planned(&dom);
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh([1.0, 1.0, 1.0])));
    // Asset 2's mesh never downloaded (v6/v7, network failure, ...).

    let (resolved, hidden) = resolve(&plan, meshes, HashMap::new());

    assert_eq!(resolved.instances.len(), 1);
    assert_eq!(resolved.instances[0].mesh, AssetRef::Id(1));
    assert_eq!(hidden, HashSet::from([Ref::new(1)]));
}

#[test]
fn a_texture_that_failed_to_download_still_resolves_the_mesh_untextured() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Rock");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    properties.insert(
        "TextureID".to_string(),
        Variant::String("rbxassetid://2".to_string()),
    );
    properties.insert("size".to_string(), vector3_variant(1.0, 1.0, 1.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh([1.0, 1.0, 1.0])));

    let (resolved, _) = resolve(&plan, meshes, HashMap::new());

    assert_eq!(resolved.instances.len(), 1);
    assert_eq!(resolved.instances[0].texture, None);
}

#[path = "tests/appearance.rs"]
mod appearance;

#[test]
fn a_force_field_mesh_part_is_capped_at_half_opaque_like_a_part() {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "MeshPart", "Shell");
    let properties = instance.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://42".to_string()),
    );
    // 1584 is `Enum.Material.ForceField` (creator-docs' Material.yaml).
    properties.insert("Material".to_string(), Variant::Enum(1584));
    properties.insert("size".to_string(), vector3_variant(4.0, 4.0, 4.0));
    properties.insert("CFrame".to_string(), cframe_at(0.0, 0.0, 0.0));
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));

    let plan = planned(&dom);

    assert_eq!(plan.entries[0].material.kind, Kind::ForceField);
    // Half-opaque is what sends it to the blended pipelines, where the
    // shell's see-through look is drawn at all.
    assert_eq!(plan.entries[0].alpha, 0.5);
}
