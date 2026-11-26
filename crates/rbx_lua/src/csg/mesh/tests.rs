use std::sync::{Mutex, MutexGuard};

use rbx_csg::glam::{DMat4, DVec3};
use rbx_dom::{Ref, Variant, WeakDom};

use super::super::{
    cframe, cframe_of, frame, has_negated_tag, local_mesh, negate_in_place, operate_in_place,
    separate_in_place, size_of, vector3, world_solid, Operation,
};
use super::{closed, mesh_uri, set_mesh_source, NON_MANIFOLD};

/// The source is process-wide: tests that set it take turns.
static SOURCE_LOCK: Mutex<()> = Mutex::new(());

/// A `version 2.00` mesh of `triangles` over `corners`.
fn mesh_bytes(corners: &[[f32; 3]], triangles: &[[u32; 3]]) -> Vec<u8> {
    let mut body = Vec::from(b"version 2.00\n".as_slice());
    body.extend_from_slice(&12u16.to_le_bytes());
    body.extend_from_slice(&[40, 12]);
    body.extend_from_slice(&(corners.len() as u32).to_le_bytes());
    body.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for corner in corners {
        for c in corner {
            body.extend_from_slice(&c.to_le_bytes());
        }
        body.extend(std::iter::repeat_n(0u8, 28));
    }
    for index in triangles.iter().flatten() {
        body.extend_from_slice(&index.to_le_bytes());
    }
    body
}

/// A cube spanning `[-1, 1]³`, every face with corners of its own as an
/// exporter splits them along UV seams, wound outward.
fn cube() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let mut corners = Vec::new();
    let mut triangles = Vec::new();
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = corners.len() as u32;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = [0.0; 3];
                p[axis] = sign;
                p[u] = a;
                p[v] = b;
                corners.push(p);
            }
            let quad = if sign > 0.0 {
                [0, 1, 2, 3]
            } else {
                [0, 3, 2, 1]
            };
            triangles.push([base + quad[0], base + quad[1], base + quad[2]]);
            triangles.push([base + quad[0], base + quad[2], base + quad[3]]);
        }
    }
    (corners, triangles)
}

/// The solid `|x| + |y| + |z| <= 1`, volume 4/3: nothing like its box.
fn octahedron() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let corners = vec![
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut triangles = Vec::new();
    for (x, y, z) in [
        (0, 2, 4),
        (2, 1, 4),
        (1, 3, 4),
        (3, 0, 4),
        (2, 0, 5),
        (1, 2, 5),
        (3, 1, 5),
        (0, 3, 5),
    ] {
        triangles.push([x, y, z]);
    }
    (corners, triangles)
}

fn flat(triangles: &[[u32; 3]]) -> Vec<u32> {
    triangles.iter().flatten().copied().collect()
}

/// Sets a source serving the cube as asset 1, the octahedron as 2 and the
/// cube with its top missing as 3, held until the guard drops.
fn with_meshes() -> MutexGuard<'static, ()> {
    let guard = SOURCE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_mesh_source(Some(Box::new(|uri| {
        let (corners, mut triangles) = match uri {
            "rbxassetid://1" | "rbxassetid://3" => cube(),
            "rbxassetid://2" => octahedron(),
            other => return Err(format!("no asset {other}")),
        };
        if uri.ends_with('3') {
            triangles.truncate(10);
        }
        Ok(mesh_bytes(&corners, &triangles))
    })));
    guard
}

fn without_meshes() -> MutexGuard<'static, ()> {
    let guard = SOURCE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_mesh_source(None);
    guard
}

fn place() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    (dom, workspace)
}

fn add(dom: &mut WeakDom, parent: Ref, class: &str, size: [f64; 3], x: f64) -> Ref {
    let r = dom.new_instance(class, class, Some(parent));
    let props = dom.get_mut(r).unwrap().properties_mut();
    props.insert("size".into(), Variant::Vector3(vector3(DVec3::from(size))));
    props.insert(
        "CFrame".into(),
        Variant::CFrame(cframe(DMat4::from_translation(DVec3::new(x, 0.0, 0.0)))),
    );
    r
}

/// A `MeshPart` drawing asset `id` (native extent 2) at `size`.
fn mesh_part(dom: &mut WeakDom, parent: Ref, id: u32, size: f64, x: f64) -> Ref {
    let r = add(dom, parent, "MeshPart", [size; 3], x);
    let props = dom.get_mut(r).unwrap().properties_mut();
    props.insert(
        "MeshId".into(),
        Variant::Content(rbx_dom::Content::Uri(format!("rbxassetid://{id}"))),
    );
    props.insert(
        "InitialSize".into(),
        Variant::Vector3(vector3(DVec3::splat(2.0))),
    );
    r
}

fn volume(dom: &WeakDom, at: Ref) -> f64 {
    let (positions, indices) = local_mesh(dom.get(at).unwrap()).unwrap();
    rbx_csg::Solid::from_triangles(&positions, &indices, DMat4::IDENTITY).volume()
}

#[test]
fn a_mesh_closes_only_when_every_edge_has_two_sides() {
    let (corners, triangles) = cube();
    assert!(closed(&corners, &flat(&triangles)));
    assert!(closed(&octahedron().0, &flat(&octahedron().1)));
    assert!(!closed(&corners, &flat(&triangles[..10])));
    assert!(!closed(&corners, &[]));
    // A sliver folded flat onto one edge of a closed cube.
    let mut folded = flat(&triangles);
    folded.extend([0, 1, 1]);
    assert!(!closed(&corners, &folded));
}

#[test]
fn a_mesh_part_carves_as_its_mesh_fitted_to_its_size() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = with_meshes();
    let (mut dom, workspace) = place();
    let gem = mesh_part(&mut dom, workspace, 2, 4.0, 0.0);
    // Scaled from extent 2 to 4: 4/3 × 2³.
    let solid = world_solid(&dom, &db, gem).unwrap();
    assert!((solid.volume() - 32.0 / 3.0).abs() < 1e-6);
    let missing = mesh_part(&mut dom, workspace, 9, 4.0, 10.0);
    let err = world_solid(&dom, &db, missing).err().unwrap();
    assert!(err.contains("no asset rbxassetid://9"), "{err}");
}

#[test]
fn without_a_source_a_mesh_part_counts_as_its_box() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = without_meshes();
    let (mut dom, workspace) = place();
    let gem = mesh_part(&mut dom, workspace, 2, 4.0, 0.0);
    assert!((world_solid(&dom, &db, gem).unwrap().volume() - 64.0).abs() < 1e-6);
}

#[test]
fn a_mesh_union_saves_separates_and_gives_the_mesh_part_back() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = with_meshes();
    let (mut dom, workspace) = place();
    let block = add(&mut dom, workspace, "Part", [6.0, 2.0, 2.0], 0.0);
    let cube = mesh_part(&mut dom, workspace, 1, 4.0, 0.0);
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[block, cube]).unwrap();
    assert_eq!(dom.get(union).unwrap().class(), "UnionOperation");
    // The 4-stud cube plus the block's two 1-stud ends.
    assert!((volume(&dom, union) - 72.0).abs() < 1e-3);
    let bytes = rbx_binary::serialize(&dom).unwrap();
    let mut back = rbx_binary::deserialize(&bytes).unwrap();
    let workspace = back.root_refs()[0];
    let union = back.get(workspace).unwrap().children()[0];
    let parts = separate_in_place(&mut back, &db, &[union]);
    let classes: Vec<&str> = parts
        .iter()
        .map(|&p| back.get(p).unwrap().class())
        .collect();
    assert_eq!(classes, ["Part", "MeshPart"]);
    let mesh = back.get(parts[1]).unwrap();
    assert_eq!(mesh_uri(mesh), Some("rbxassetid://1"));
    assert!((size_of(&db, mesh) - DVec3::splat(4.0)).length() < 1e-5);
    assert!(frame(&cframe_of(&db, mesh)).w_axis.truncate().length() < 1e-5);
}

#[test]
fn a_negated_mesh_part_carves_its_shape_and_comes_back_negated() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = with_meshes();
    let (mut dom, workspace) = place();
    let block = add(&mut dom, workspace, "Part", [6.0, 6.0, 6.0], 0.0);
    let gem = mesh_part(&mut dom, workspace, 2, 4.0, 0.0);
    let gem = negate_in_place(&mut dom, &db, &[gem])[0];
    let union = operate_in_place(&mut dom, &db, Operation::Union, &[block, gem]).unwrap();
    assert!((volume(&dom, union) - (216.0 - 32.0 / 3.0)).abs() < 1e-3);
    // Kept as the tagged MeshPart, not in a `NegateOperation`: Studio
    // refuses to separate that one.
    let tree = super::super::shared_bytes(dom.get(union).unwrap(), "ChildData2").unwrap();
    let tree = rbx_binary::deserialize(tree).unwrap();
    let leaf = tree.get(tree.root_refs()[1]).unwrap();
    assert_eq!(leaf.class(), "MeshPart");
    assert!(has_negated_tag(leaf));
    let parts = separate_in_place(&mut dom, &db, &[union]);
    let gem = dom.get(parts[1]).unwrap();
    assert_eq!(gem.class(), "MeshPart");
    assert!(has_negated_tag(gem));
}

#[test]
fn an_open_mesh_fails_in_studios_words_and_leaves_the_place_alone() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = with_meshes();
    let (mut dom, workspace) = place();
    let block = add(&mut dom, workspace, "Part", [6.0, 2.0, 2.0], 0.0);
    let open = mesh_part(&mut dom, workspace, 3, 4.0, 0.0);
    let err = operate_in_place(&mut dom, &db, Operation::Union, &[block, open]).unwrap_err();
    assert_eq!(err, NON_MANIFOLD);
    assert_eq!(dom.get(workspace).unwrap().children(), [block, open]);
}

#[test]
fn studios_saved_mesh_union_counts_as_its_box() {
    // Studio's own mesh union is a `MeshPart` whose mesh lives only in
    // memory: its file names no mesh, and it reopens as a box.
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let _meshes = with_meshes();
    let (mut dom, workspace) = place();
    let saved = add(&mut dom, workspace, "MeshPart", [6.0, 4.0, 4.0], 0.0);
    assert!((world_solid(&dom, &db, saved).unwrap().volume() - 96.0).abs() < 1e-6);
}
