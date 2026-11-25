//! `SpecialMesh` Head planning, and which `SpecialMesh` a `VertexColor`
//! tints, on the same fixtures the geometry tests use.

use super::*;

/// A part of `size` in a workspace, wearing one `SpecialMesh` of `mesh_type`
/// with `extra` properties.
fn wearing(size: [f32; 3], mesh_type: u32, extra: Vec<(&str, Variant)>) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let part_ref = Ref::new(1);
    let mut part = Instance::new(part_ref, "Part", "Head");
    part.properties_mut().insert(
        "size".to_string(),
        vector3_variant(size[0], size[1], size[2]),
    );
    part.properties_mut()
        .insert("CFrame".to_string(), cframe_at(0.0, 5.0, 0.0));
    part.properties_mut().insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    );
    dom.insert(part);
    dom.set_parent(part_ref, Some(workspace));
    let mut mesh = Instance::new(Ref::new(2), "SpecialMesh", "Mesh");
    mesh.properties_mut()
        .insert("MeshType".to_string(), Variant::Enum(mesh_type));
    for (name, value) in extra {
        mesh.properties_mut().insert(name.to_string(), value);
    }
    dom.insert(mesh);
    dom.set_parent(Ref::new(2), Some(part_ref));
    dom
}

#[test]
fn a_head_draws_studios_head_mesh_fitted_to_the_head_extent() {
    let dom = wearing(
        [2.0, 1.0, 1.0],
        HEAD_MESH,
        vec![
            ("Scale", vector3_variant(1.25, 1.25, 1.25)),
            ("Offset", vector3_variant(0.0, 0.5, 0.0)),
        ],
    );
    let plan = planned(&dom);

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].mesh,
        AssetRef::Native("avatar/heads/head.mesh".to_string())
    );
    // The classic 2x1x1 head at Scale 1.25: a 1.25 stud head extent, which
    // head.mesh's own 1.2 stud span fills at Studio's 0.96.
    let transform = plan.entries[0].fit.transform(&fake_mesh([1.2, 1.2, 1.2]));
    assert!(transform
        .transform_vector3(Vec3::splat(1.2))
        .abs_diff_eq(Vec3::splat(1.2), 1e-5));
    assert!(transform
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(0.0, 5.5, 0.0), 1e-5));
}

#[test]
fn a_head_ignores_its_mesh_id() {
    let dom = wearing(
        [4.0, 4.0, 4.0],
        HEAD_MESH,
        vec![("MeshId", Variant::String("rbxassetid://7".to_string()))],
    );
    assert_eq!(
        planned(&dom).entries[0].mesh,
        AssetRef::Native("avatar/heads/head.mesh".to_string())
    );
}

#[test]
fn vertex_color_tints_a_textured_mesh_only() {
    let green = ("VertexColor", vector3_variant(0.0, 1.0, 0.0));
    let plain = wearing([4.0, 4.0, 4.0], HEAD_MESH, vec![green.clone()]);
    assert_eq!(planned(&plain).entries[0].color_for(false), [1.0, 0.0, 0.0]);

    let textured = wearing(
        [4.0, 4.0, 4.0],
        HEAD_MESH,
        vec![
            green,
            ("TextureId", Variant::String("rbxassetid://8".to_string())),
        ],
    );
    // The image takes the tint, not the part's red: a textured mesh is
    // drawn in its image's own colours.
    let entry = &planned(&textured).entries[0];
    assert_eq!(entry.color_for(true), [0.0, 1.0, 0.0]);
    assert_eq!(entry.color_for(false), [1.0, 0.0, 0.0]);
}
