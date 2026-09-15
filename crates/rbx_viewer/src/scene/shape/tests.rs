use super::*;
use rbx_dom::{Ref, Vector3Data};

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

fn part(dom: &mut WeakDom, class: &str) -> Ref {
    let referent = Ref::new(dom.root_refs().len() as u32 + 1);
    let instance = Instance::new(referent, class, class);
    dom.insert(instance);
    dom.set_parent(referent, None);
    referent
}

fn set_vector3(dom: &mut WeakDom, referent: Ref, property: &str, v: [f32; 3]) {
    dom.get_mut(referent).unwrap().properties_mut().insert(
        property.to_string(),
        Variant::Vector3(Vector3Data {
            x: v[0],
            y: v[1],
            z: v[2],
        }),
    );
}

#[test]
fn a_plain_part_with_no_shape_property_is_a_box() {
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "Part");

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::new(4.0, 2.0, 6.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Box);
    assert_eq!(geometry.size, Vec3::new(4.0, 2.0, 6.0));
}

#[test]
fn part_shape_ball_clamps_to_the_smallest_dimension() {
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "Part");
    dom.get_mut(referent)
        .unwrap()
        .properties_mut()
        .insert("shape".to_string(), Variant::Enum(0));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::new(4.0, 2.0, 6.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Ball);
    assert_eq!(geometry.size, Vec3::splat(2.0));
}

#[test]
fn part_shape_cylinder_is_cylinder_x_with_the_parts_own_size() {
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "Part");
    dom.get_mut(referent)
        .unwrap()
        .properties_mut()
        .insert("shape".to_string(), Variant::Enum(2));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::new(4.0, 2.0, 2.0),
    );

    assert_eq!(geometry.kind, ShapeKind::CylinderX);
    assert_eq!(geometry.size, Vec3::new(4.0, 2.0, 2.0));
}

#[test]
fn a_particle_emitter_shape_property_is_never_confused_for_a_part_shape() {
    // A different Shape enum (ParticleEmitterShape) on an unrelated class
    // must never be read as if it were Part's lowercase `shape`.
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "ParticleEmitter");
    dom.get_mut(referent)
        .unwrap()
        .properties_mut()
        .insert("Shape".to_string(), Variant::Enum(0));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::splat(4.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Box);
}

#[test]
fn wedge_part_is_a_wedge_regardless_of_any_shape_property() {
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "WedgePart");

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::splat(4.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Wedge);
}

#[test]
fn corner_wedge_part_is_a_corner_wedge() {
    let mut dom = WeakDom::new();
    let referent = part(&mut dom, "CornerWedgePart");

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(referent).unwrap(),
        Vec3::splat(4.0),
    );

    assert_eq!(geometry.kind, ShapeKind::CornerWedge);
}

#[test]
fn a_cylinder_mesh_child_replaces_the_parent_with_a_y_axis_cylinder() {
    let mut dom = WeakDom::new();
    let part_ref = part(&mut dom, "Part");
    let mesh_ref = part(&mut dom, "CylinderMesh");
    dom.set_parent(mesh_ref, Some(part_ref));
    set_vector3(&mut dom, mesh_ref, "Scale", [2.0, 3.0, 2.0]);
    set_vector3(&mut dom, mesh_ref, "Offset", [0.0, 1.0, 0.0]);

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(part_ref).unwrap(),
        Vec3::new(1.0, 4.0, 1.0),
    );

    assert_eq!(geometry.kind, ShapeKind::CylinderY);
    assert_eq!(geometry.size, Vec3::new(2.0, 12.0, 2.0));
    assert_eq!(geometry.offset, Vec3::new(0.0, 1.0, 0.0));
}

#[test]
fn a_block_mesh_child_keeps_a_box_but_still_applies_scale() {
    let mut dom = WeakDom::new();
    let part_ref = part(&mut dom, "Part");
    let mesh_ref = part(&mut dom, "BlockMesh");
    dom.set_parent(mesh_ref, Some(part_ref));
    set_vector3(&mut dom, mesh_ref, "Scale", [2.0, 2.0, 2.0]);

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(part_ref).unwrap(),
        Vec3::splat(1.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Box);
    assert_eq!(geometry.size, Vec3::splat(2.0));
}

#[test]
fn a_special_mesh_sphere_is_an_ellipsoid_not_clamped_like_part_shape_ball() {
    let mut dom = WeakDom::new();
    let part_ref = part(&mut dom, "Part");
    let mesh_ref = part(&mut dom, "SpecialMesh");
    dom.set_parent(mesh_ref, Some(part_ref));
    dom.get_mut(mesh_ref)
        .unwrap()
        .properties_mut()
        .insert("MeshType".to_string(), Variant::Enum(3));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(part_ref).unwrap(),
        Vec3::new(4.0, 2.0, 6.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Ball);
    assert_eq!(geometry.size, Vec3::new(4.0, 2.0, 6.0));
}

#[test]
fn a_special_mesh_file_mesh_leaves_the_box_untouched() {
    let mut dom = WeakDom::new();
    let part_ref = part(&mut dom, "Part");
    let mesh_ref = part(&mut dom, "SpecialMesh");
    dom.set_parent(mesh_ref, Some(part_ref));
    dom.get_mut(mesh_ref)
        .unwrap()
        .properties_mut()
        .insert("MeshType".to_string(), Variant::Enum(5));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(part_ref).unwrap(),
        Vec3::splat(4.0),
    );

    assert_eq!(geometry.kind, ShapeKind::Box);
    assert_eq!(geometry.size, Vec3::splat(4.0));
}

#[test]
fn a_corner_wedge_mesh_type_maps_to_the_corner_wedge_shape() {
    let mut dom = WeakDom::new();
    let part_ref = part(&mut dom, "Part");
    let mesh_ref = part(&mut dom, "SpecialMesh");
    dom.set_parent(mesh_ref, Some(part_ref));
    dom.get_mut(mesh_ref)
        .unwrap()
        .properties_mut()
        .insert("MeshType".to_string(), Variant::Enum(11));

    let geometry = resolve(
        &dom,
        &database(),
        dom.get(part_ref).unwrap(),
        Vec3::splat(4.0),
    );

    assert_eq!(geometry.kind, ShapeKind::CornerWedge);
}
