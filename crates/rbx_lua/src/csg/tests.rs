use rbx_dom::Variant;

use super::*;

fn part(dom: &mut WeakDom, parent: Ref, size: [f64; 3], x: f64) -> Ref {
    let r = dom.new_instance("Part", "Part", Some(parent));
    let props = dom.get_mut(r).unwrap().properties_mut();
    props.insert("size".into(), Variant::Vector3(vector3(DVec3::from(size))));
    props.insert("Anchored".into(), Variant::Bool(true));
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
    let (positions, indices) = local_mesh(instance).expect("SolidMeshHolder decodes");
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
    // Studio's Separate gives the subtracted part back as itself, negated.
    assert_eq!(classes, ["Part", "Part"]);
    let hole = back.get(parts[1]).unwrap();
    assert!((frame(&cframe_of(&db, hole)).w_axis.x - 1.0).abs() < 1e-5);
    assert!(has_negated_tag(hole));
    assert_eq!(
        read(&db, hole, "Transparency"),
        Some(&Variant::Float32(0.5))
    );
}

#[test]
fn carving_again_drops_studios_cached_geometry() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [2.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 1.0);
    let stale = |dom: &mut WeakDom, at: Ref| {
        let props = dom.get_mut(at).unwrap().properties_mut();
        for key in STALE_GEOMETRY {
            props.insert(key.into(), shared(b"stale".to_vec()));
        }
    };
    let cached = |dom: &WeakDom, at: Ref| {
        let props = dom.get(at).unwrap().properties();
        STALE_GEOMETRY
            .iter()
            .any(|k| props.get(*k) == Some(&shared(b"stale".to_vec())))
            || props.contains_key("MeshData2")
    };
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, b]).unwrap();
    stale(&mut dom, union);
    let c = part(&mut dom, workspace, [2.0, 2.0, 2.0], 3.0);
    let again = operate_in_place(&mut dom, &db, Operation::Union, &[union, c]).unwrap();
    assert!(!cached(&dom, again));
    stale(&mut dom, again);
    let negate = negate_operations_in_place(&mut dom, &db, &[again])[0];
    assert!(!cached(&dom, negate));
}

fn tags(dom: &WeakDom, at: Ref) -> Vec<String> {
    rbx_dom::attributes::tags(dom.get(at).unwrap().properties().get("Tags"))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// What Studio's Negate button does now (observed in Studio 2026-11-23):
/// the part stays a part, tagged `rbxNegated` at `Transparency` 0.5;
/// pressing it again on a negated part untags it and makes it opaque.
#[test]
fn negate_tags_the_part_and_toggles_back() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [2.0, 2.0, 2.0], 0.0);
    set(&mut dom, &db, a, "Transparency", Variant::Float32(0.3));
    assert_eq!(negate_in_place(&mut dom, &db, &[a]), [a]);
    let instance = dom.get(a).unwrap();
    assert_eq!(instance.class(), "Part");
    assert_eq!(tags(&dom, a), [NEGATED_TAG]);
    assert_eq!(
        read(&db, instance, "Transparency"),
        Some(&Variant::Float32(0.5))
    );
    negate_in_place(&mut dom, &db, &[a]);
    assert!(tags(&dom, a).is_empty());
    assert_eq!(
        read(&db, dom.get(a).unwrap(), "Transparency"),
        Some(&Variant::Float32(0.0))
    );
}

/// A union keeps a negated part the way Studio's tree does: a
/// "NegativePart" `NegateOperation` with no mesh, the part untagged in it.
#[test]
fn a_negated_part_is_wrapped_like_studio_wraps_it() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [4.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 1.0);
    negate_in_place(&mut dom, &db, &[b]);
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, b]).unwrap();
    assert!((mesh_volume(dom.get(union).unwrap()) - 8.0).abs() < 1e-3);
    let tree =
        rbx_binary::deserialize(shared_bytes(dom.get(union).unwrap(), "ChildData2").unwrap())
            .unwrap();
    let wrapper = tree.get(tree.root_refs()[1]).unwrap();
    assert_eq!(
        (wrapper.class(), wrapper.name()),
        ("NegateOperation", "NegativePart")
    );
    assert!(shared_bytes(wrapper, "SolidMeshHolder").is_none());
    assert_eq!(
        read(&db, wrapper, "Transparency"),
        Some(&Variant::Float32(0.1))
    );
    let inner = rbx_binary::deserialize(shared_bytes(wrapper, "ChildData2").unwrap()).unwrap();
    let inner = inner.get(inner.root_refs()[0]).unwrap();
    assert_eq!(inner.class(), "Part");
    assert!(!has_negated_tag(inner));
    assert_eq!(
        read(&db, inner, "Transparency"),
        Some(&Variant::Float32(0.5))
    );
}

fn fidelity_of(dom: &WeakDom, at: Ref) -> Option<Fidelity> {
    csgphs::fidelity(shared_bytes(dom.get(at).unwrap(), "PhysicalConfigData")?)
}

#[test]
fn a_union_collides_by_its_pieces_and_follows_its_fidelity() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [6.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 6.0], 2.0);
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, b]).unwrap();
    let blob = shared_bytes(dom.get(union).unwrap(), "PhysicalConfigData").unwrap();
    let decoded = csgphs::decode(blob).unwrap();
    assert_eq!(decoded.fidelity, Fidelity::Default);
    assert_eq!(decoded.hulls.len(), 2, "a T is two boxes");
    assert!(!refresh_collision(&mut dom, &db, union), "already Default");
    for fidelity in [
        Fidelity::Hull,
        Fidelity::Box,
        Fidelity::PreciseConvexDecomposition,
    ] {
        set(
            &mut dom,
            &db,
            union,
            "CollisionFidelity",
            Variant::Enum(fidelity.value()),
        );
        assert!(refresh_collision(&mut dom, &db, union));
        assert_eq!(fidelity_of(&dom, union), Some(fidelity));
    }
    // A plain part has no collision blob to refresh.
    assert!(!refresh_collision(&mut dom, &db, workspace));
}

fn painted(dom: &mut WeakDom, at: Ref, rgb: [u8; 3]) {
    let props = dom.get_mut(at).unwrap().properties_mut();
    props.insert(
        "Color3uint8".into(),
        Variant::Color3uint8 {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
        },
    );
}

fn vector(instance: &Instance, name: &str) -> DVec3 {
    match instance.properties().get(name) {
        Some(Variant::Vector3(v)) => vec3(*v),
        other => panic!("{name}: {other:?}"),
    }
}

/// What Studio saves on a union it makes: the mesh in `SolidMeshHolder`,
/// each face its part's colour, `MeshData2` empty, and the mass beside it.
#[test]
fn a_union_saves_its_mesh_where_studio_reads_it() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let a = part(&mut dom, workspace, [4.0, 2.0, 2.0], 0.0);
    let b = part(&mut dom, workspace, [2.0, 2.0, 2.0], 2.0);
    painted(&mut dom, a, [255, 0, 0]);
    painted(&mut dom, b, [0, 255, 0]);
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[a, b]).unwrap();
    let instance = dom.get(union).unwrap();
    assert!(!instance.properties().contains_key("MeshData2"));
    let mesh = solid_mesh(instance).expect("SolidMeshHolder");
    let mut colors = mesh.colors.clone();
    colors.sort();
    colors.dedup();
    assert_eq!(colors, [[0, 255, 0], [255, 0, 0]]);
    assert_eq!(
        instance.properties().get("TriangleCount"),
        Some(&Variant::Int32(mesh.triangles.len() as i32))
    );
    // A 5×2×2 box, centred on itself.
    assert_eq!(
        instance.properties().get("UnscaledVolume"),
        Some(&Variant::Float32(20.0))
    );
    assert!(vector(instance, "UnscaledCofm").length() < 1e-5);
    let diagonal = vector(instance, "UnscaledVolInertiaDiags");
    let expected = DVec3::new(8.0, 29.0, 29.0) * 20.0 / 12.0;
    assert!(
        (diagonal - expected).abs().max_element() < 1e-3,
        "{diagonal}"
    );
}

const NESTED_UNION: &str = include_str!("../../../../assets/tests/csg/NestedUnion.rbxmx");
const NEGATED_HOLE: &str = include_str!("../../../../assets/tests/csg/NegatedHoleUnion.rbxmx");
const TWO_CUBES: &[u8] = include_bytes!("../../../../assets/tests/csg/TwoCubesUnion.rbxm");

/// A place holding the one instance `model` does, in its `Workspace`.
fn studio_place(mut model: WeakDom) -> (WeakDom, Ref, Ref) {
    let root = model.root_refs()[0];
    let workspace = model.new_instance("Workspace", "Workspace", None);
    model.set_parent(root, Some(workspace));
    (model, workspace, root)
}

fn position(db: &ReflectionDatabase, dom: &WeakDom, at: Ref) -> DVec3 {
    vec3(cframe_of(db, dom.get(at).unwrap()).position)
}

#[test]
fn a_studio_union_separates_into_its_parts() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, _, union) = studio_place(rbx_binary::deserialize(TWO_CUBES).unwrap());
    let parts = separate_in_place(&mut dom, &db, &[union]);
    let mut xs: Vec<f64> = parts.iter().map(|&p| position(&db, &dom, p).x).collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(parts.len(), 2);
    assert!(
        (xs[0] + 12.0).abs() < 1e-4 && (xs[1] + 10.0).abs() < 1e-4,
        "{xs:?}"
    );
    assert!(parts.iter().all(|&p| dom.get(p).unwrap().class() == "Part"));
}

/// Studio leaves the operations nested in a union without a mesh; Separate
/// carves them again so they draw in Studio, and they separate again too.
#[test]
fn separating_a_studio_union_rebakes_the_operations_inside() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, _, union) = studio_place(rbx_xml::deserialize(NESTED_UNION).unwrap());
    let pieces = separate_in_place(&mut dom, &db, &[union]);
    let mut by_class: Vec<(&str, f64)> = pieces
        .iter()
        .map(|&p| {
            let instance = dom.get(p).unwrap();
            (instance.class(), mesh_volume(instance))
        })
        .collect();
    by_class.sort_by(|a, b| a.0.cmp(b.0));
    // Studio's own volumes for the two (`UnscaledVolume`); its ball is
    // tessellated a little differently.
    assert_eq!(by_class[0].0, "IntersectOperation");
    assert!((by_class[0].1 - 43.7749).abs() < 1.0, "{by_class:?}");
    assert_eq!(by_class[1].0, "UnionOperation");
    assert!((by_class[1].1 - 28.0356).abs() < 1e-3, "{by_class:?}");
    let union = pieces
        .iter()
        .copied()
        .find(|&p| dom.get(p).unwrap().class() == "UnionOperation")
        .unwrap();
    assert_eq!(separate_in_place(&mut dom, &db, &[union]).len(), 2);
}

/// A Studio union carries only `SolidMeshHolder`: carving it again reads
/// that, not its box, and keeps the colours of the parts it was made of.
#[test]
fn a_studio_union_carves_again_by_its_mesh() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace, hole) = studio_place(rbx_xml::deserialize(NEGATED_HOLE).unwrap());
    let volume = solid_mesh(dom.get(hole).unwrap())
        .unwrap()
        .solid(DMat4::IDENTITY)
        .volume();
    // A 4×2×4 block less a 6×1×1 cylinder through it.
    assert!(volume < 32.0 && volume > 28.0, "{volume}");
    let far = part(&mut dom, workspace, [2.0, 2.0, 2.0], 100.0);
    let again = operate_in_place(&mut dom, &db, Operation::Union, &[hole, far]).unwrap();
    let instance = dom.get(again).unwrap();
    assert!((mesh_volume(instance) - (volume + 8.0)).abs() < 1e-2);
    let colors = solid_mesh(instance).unwrap().colors;
    assert!(colors.contains(&[0, 0, 255]) && colors.contains(&[200, 200, 0]));
}

/// Studio draws a small stand-in box for a `SolidMeshHolder` that is not
/// closed: a box cut by a ball, whose boolean leaves slivers, must still
/// save every edge with its twin.
#[test]
fn a_curved_result_saves_a_closed_mesh() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    for operation in [Operation::Intersect, Operation::Union] {
        let (mut dom, workspace) = place();
        let a = part(&mut dom, workspace, [4.0, 4.0, 4.0], 8.0);
        let b = part(&mut dom, workspace, [5.0, 5.0, 5.0], 9.0);
        dom.get_mut(b)
            .unwrap()
            .properties_mut()
            .insert("Shape".into(), Variant::Enum(0));
        let made = operate_in_place(&mut dom, &db, operation, &[a, b]).unwrap();
        let mesh = solid_mesh(dom.get(made).unwrap()).unwrap();
        let mut edges = std::collections::HashMap::new();
        for t in &mesh.triangles {
            for edge in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry(edge).or_insert(0) += 1;
            }
        }
        let open = edges
            .iter()
            .filter(|&(&(u, v), &n)| n != 1 || edges.get(&(v, u)) != Some(&1))
            .count();
        assert_eq!(open, 0, "{operation:?}");
    }
}

fn place_at(dom: &mut WeakDom, db: &ReflectionDatabase, at: Ref, frame: DMat4) {
    set(dom, db, at, "CFrame", Variant::CFrame(cframe(frame)));
}

/// An 8×4×8 block with a 3-stud hole straight down through it, at `x`.
fn holed_block(dom: &mut WeakDom, db: &ReflectionDatabase, workspace: Ref, x: f64) -> Ref {
    let block = part(dom, workspace, [8.0, 4.0, 8.0], x);
    let drill = part(dom, workspace, [6.0, 3.0, 3.0], x);
    let upright = DMat4::from_translation(DVec3::new(x, 0.0, 0.0))
        * DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
    place_at(dom, db, drill, upright);
    set(dom, db, drill, "Shape", Variant::Enum(2));
    negate_in_place(dom, db, &[drill]);
    operate_in_place(dom, db, Operation::Union, &[block, drill]).unwrap()
}

/// Set `RBX_CSG_COLLISION_PLACE` to keep the place this writes: one holed
/// block per fidelity on a floor, a ball over each hole and a cube over
/// each corner, to open in Studio and run.
#[test]
fn holed_blocks_at_every_fidelity() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let (mut dom, workspace) = place();
    let floor = part(&mut dom, workspace, [200.0, 2.0, 60.0], 0.0);
    place_at(
        &mut dom,
        &db,
        floor,
        DMat4::from_translation(DVec3::new(30.0, -3.0, 0.0)),
    );
    let fidelities = [
        Fidelity::Box,
        Fidelity::Hull,
        Fidelity::Default,
        Fidelity::PreciseConvexDecomposition,
    ];
    for (i, fidelity) in fidelities.into_iter().enumerate() {
        let x = 20.0 * i as f64;
        let union = holed_block(&mut dom, &db, workspace, x);
        dom.set_name(union, &format!("Hole_{fidelity:?}")).unwrap();
        let value = Variant::Enum(fidelity.value());
        set(&mut dom, &db, union, "CollisionFidelity", value);
        refresh_collision(&mut dom, &db, union);
        assert_eq!(fidelity_of(&dom, union), Some(fidelity));
        for (name, shape, at) in [
            ("Ball", 0, DVec3::new(x, 8.0, 0.0)),
            ("Cube", 1, DVec3::new(x + 2.5, 8.0, 2.5)),
        ] {
            let drop = part(&mut dom, workspace, [2.0, 2.0, 2.0], 0.0);
            dom.set_name(drop, &format!("{name}_{fidelity:?}")).unwrap();
            set(&mut dom, &db, drop, "Shape", Variant::Enum(shape));
            set(&mut dom, &db, drop, "Anchored", Variant::Bool(false));
            place_at(&mut dom, &db, drop, DMat4::from_translation(at));
        }
    }
    if let Some(path) = std::env::var_os("RBX_CSG_COLLISION_PLACE") {
        std::fs::write(path, rbx_binary::serialize(&dom).unwrap()).unwrap();
    }
}
