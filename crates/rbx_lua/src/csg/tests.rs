use rbx_dom::Variant;

use super::*;

fn part(dom: &mut WeakDom, parent: Ref, size: [f64; 3], x: f64) -> Ref {
    let r = dom.new_instance("Part", "Part", Some(parent));
    let props = dom.get_mut(r).unwrap().properties_mut();
    props.insert("size".into(), Variant::Vector3(vector3(DVec3::from(size))));
    props.insert(
        "CFrame".into(),
        Variant::CFrame(cframe(DMat4::from_translation(DVec3::new(x, 0.0, 0.0)))),
    );
    r
}

fn place() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    (dom, workspace)
}

fn mesh_volume(instance: &Instance) -> f64 {
    let (positions, indices) = local_mesh(instance).expect("MeshData2 decodes");
    Solid::from_triangles(&positions, &indices, DMat4::IDENTITY).volume()
}

#[test]
fn a_union_is_centred_on_its_bounds_with_the_real_volume() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [2.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 1.0);
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, b]).unwrap();
    let instance = dom.get(union).unwrap();
    assert_eq!(
        (instance.class(), instance.name()),
        ("UnionOperation", "Union")
    );
    assert!((size_of(&db, instance) - DVec3::new(3.0, 2.0, 2.0)).length() < 1e-5);
    assert!((frame(&cframe_of(&db, instance)).w_axis.x - 0.5).abs() < 1e-5);
    assert!((mesh_volume(instance) - 12.0).abs() < 1e-3);
}

#[test]
fn a_subtraction_and_an_intersection_carve() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [4.0, 4.0, 4.0], 0.0);
    let b = part(&mut dom, workspace, [4.0, 4.0, 4.0], 2.0);
    let cut = combine(&dom, &db, Operation::Subtract, a, &[b]).unwrap();
    assert!((cut.volume() - 32.0).abs() < 1e-6);
    let inter = combine(&dom, &db, Operation::Intersect, a, &[b]).unwrap();
    assert!((inter.volume() - 32.0).abs() < 1e-6);
    let far = part(&mut dom, workspace, [1.0, 1.0, 1.0], 20.0);
    assert_eq!(
        combine(&dom, &db, Operation::Intersect, a, &[far])
            .err()
            .as_deref(),
        Some("Empty")
    );
}

#[test]
fn a_service_result_keeps_the_base_frame() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [2.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 6.0);
    let solid = combine(&dom, &db, Operation::Union, a, &[b]).unwrap();
    let made = build(
        &mut dom,
        &db,
        Operation::Union,
        Flavor::Service,
        a,
        &[b],
        solid,
        true,
    );
    assert_eq!(made.len(), 2);
    let far = dom.get(made[1].referent).unwrap();
    assert_eq!(
        (far.class(), far.name()),
        ("PartOperation", "PartOperation")
    );
    assert!(frame(&cframe_of(&db, far)).w_axis.truncate().length() < 1e-6);
    assert!(read(&db, far, "ChildData2").is_none());
}

/// Set `RBX_CSG_SAVE_PLACE` to keep the place this writes, to open in Studio.
#[test]
fn a_union_survives_the_place_writer_and_separates_back() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [4.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 1.0);
    let negate = negate_in_place(&mut dom, &db, &[b])[0];
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, negate]).unwrap();
    let bytes = rbx_binary::serialize(&dom).unwrap();
    if let Some(path) = std::env::var_os("RBX_CSG_SAVE_PLACE") {
        std::fs::write(path, &bytes).unwrap();
    }
    let mut back = rbx_binary::deserialize(&bytes).unwrap();
    let workspace = back.root_refs()[0];
    let union_back = back.get(workspace).unwrap().children()[0];
    let instance = back.get(union_back).unwrap();
    assert_eq!(instance.class(), "UnionOperation");
    assert!((mesh_volume(instance) - 8.0).abs() < 1e-3);
    assert!(dom.get(union).is_some());
    let parts = separate_in_place(&mut back, &db, &[union_back]);
    let classes: Vec<&str> = parts
        .iter()
        .map(|&p| back.get(p).unwrap().class())
        .collect();
    assert_eq!(classes, ["Part", "NegateOperation"]);
}
