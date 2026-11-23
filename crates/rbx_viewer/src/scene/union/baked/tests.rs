//! `MeshData` blobs built here in the layout the module doc describes, no
//! Roblox blob is committed (see `CONTRIBUTING.md`); the one real check is
//! the ignored fixture test at the bottom. The `SolidMeshHolder` tests read
//! what Studio saved for two primitive parts made for them.

use std::collections::HashMap;

use rbx_dom::{Instance, Ref, Variant, WeakDom};

use super::super::tests::{database, dom_with, operation};
use super::super::tests_support::{asset_bytes, fallback_leaves, inline_bytes};
use super::super::{fit, plan, resolve, Catalog, Evaluations};
use super::*;

/// A stride bigger than the 28 bytes read, as real blobs have (84).
const STRIDE: u32 = 40;

struct Corner {
    position: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
}

fn corner(position: [f32; 3], normal: [f32; 3], color: [u8; 3]) -> Corner {
    Corner {
        position,
        normal,
        color: [color[0], color[1], color[2], 255],
    }
}

/// The plain version-`version` document holding `corners` and `indices`.
fn document(version: u32, stride: u32, corners: &[Corner], indices: &[u32]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend(version.to_le_bytes());
    out.extend(b"0123456789abcdef");
    out.extend([7u8; 16]);
    out.extend((corners.len() as u32).to_le_bytes());
    out.extend(stride.to_le_bytes());
    for corner in corners {
        let start = out.len();
        for value in corner.position.iter().chain(&corner.normal) {
            out.extend(value.to_le_bytes());
        }
        out.extend(corner.color);
        out.resize(start + stride as usize, 0);
    }
    out.extend((indices.len() as u32).to_le_bytes());
    for index in indices {
        out.extend(index.to_le_bytes());
    }
    out
}

fn scrambled(plain: &[u8]) -> Vec<u8> {
    plain
        .iter()
        .zip(KEY.iter().cycle())
        .map(|(byte, key)| byte ^ key)
        .collect()
}

/// Two triangles facing +Z: a big red one and a small blue one.
fn two_triangles() -> Vec<u8> {
    let up = [0.0, 0.0, 1.0];
    let red = [200, 10, 10];
    let blue = [10, 10, 200];
    let corners = [
        corner([-2.0, -2.0, 0.5], up, red),
        corner([2.0, -2.0, 0.5], up, red),
        corner([0.0, 2.0, 0.5], up, red),
        corner([0.0, 0.0, 0.5], up, blue),
        corner([0.1, 0.0, 0.5], up, blue),
        corner([0.0, 0.1, 0.5], up, blue),
    ];
    document(2, STRIDE, &corners, &[0, 1, 2, 3, 4, 5])
}

#[test]
fn a_scrambled_blob_decodes_like_its_plain_form() {
    let plain = two_triangles();
    let from_plain = decode(&plain).expect("plain decodes");
    let from_scrambled = decode(&scrambled(&plain)).expect("scrambled decodes");

    assert_eq!(from_plain.mesh.vertices, from_scrambled.mesh.vertices);
    assert_eq!(from_plain.mesh.indices, from_scrambled.mesh.indices);
}

#[test]
fn positions_normals_and_bounds_come_through_as_stored() {
    let baked = decode(&scrambled(&two_triangles())).expect("decodes");
    let mesh = &baked.mesh;

    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(mesh.indices, [0, 1, 2, 3, 4, 5]);
    assert_eq!(mesh.vertices[1].position, [2.0, -2.0, 0.5]);
    assert_eq!(mesh.vertices[1].normal, [0.0, 0.0, 1.0]);
    assert_eq!(mesh.bounds.min, [-2.0, -2.0, 0.5]);
    assert_eq!(mesh.bounds.max, [2.0, 2.0, 0.5]);
}

#[test]
fn the_colour_covering_most_area_paints_it_and_the_vertices_go_white() {
    let baked = decode(&two_triangles()).expect("decodes");

    assert_eq!(
        baked.color,
        Some([200, 10, 10]),
        "the big red triangle wins"
    );
    assert!(baked.mesh.vertices.iter().all(|v| v.color == [255; 4]));
}

#[test]
fn texture_coordinates_are_box_projected_like_a_carved_union() {
    let baked = decode(&two_triangles()).expect("decodes");
    let tile = rbx_materials::DEFAULT_STUDS_PER_TILE;

    // Facing +Z, so projected along X and Y.
    assert_eq!(baked.mesh.vertices[1].uv, [2.0 / tile, -2.0 / tile]);
}

#[test]
fn bytes_that_are_not_csgmdl_either_way_are_refused() {
    assert_eq!(decode(b"definitely not a mesh").err(), Some(Error::NotCsg));
    assert_eq!(decode(&[]).err(), Some(Error::NotCsg));
}

#[test]
fn another_version_is_named_rather_than_misread() {
    let blob = scrambled(&document(7, STRIDE, &[], &[]));

    assert_eq!(decode(&blob).err(), Some(Error::Version(7)));
}

#[test]
fn every_truncation_is_an_error_never_a_panic() {
    let plain = two_triangles();
    for len in 0..plain.len() {
        assert!(decode(&plain[..len]).is_err(), "cut at {len}");
        assert!(decode(&scrambled(&plain[..len])).is_err(), "cut at {len}");
    }
}

#[test]
fn counts_too_large_for_the_blob_are_truncation() {
    let mut plain = two_triangles();
    // The vertex count, after the magic, version and two digests.
    plain[42..46].copy_from_slice(&u32::MAX.to_le_bytes());

    assert_eq!(decode(&plain).err(), Some(Error::Truncated));
}

#[test]
fn a_vertex_too_small_to_read_is_refused() {
    let blob = document(2, 12, &[], &[]);

    assert_eq!(decode(&blob).err(), Some(Error::Stride(12)));
}

#[test]
fn an_index_past_the_last_vertex_is_refused() {
    let up = [0.0, 0.0, 1.0];
    let corners = [corner([0.0; 3], up, [0; 3]), corner([1.0; 3], up, [0; 3])];
    let blob = document(2, STRIDE, &corners, &[0, 1, 2]);

    assert_eq!(decode(&blob).err(), Some(Error::Index));
}

#[test]
fn indices_that_are_not_whole_triangles_are_refused() {
    let up = [0.0, 0.0, 1.0];
    let corners = [corner([0.0; 3], up, [0; 3]), corner([1.0; 3], up, [0; 3])];
    let blob = document(2, STRIDE, &corners, &[0, 1]);

    assert_eq!(decode(&blob).err(), Some(Error::Index));
}

/// [`two_triangles`] as version 4, each triangle its own LOD, with the
/// trailing count and offsets the real blob ends on (`3, 0, 1050, 1152`).
fn two_lods() -> Vec<u8> {
    let mut plain = two_triangles();
    plain[MAGIC.len()] = 4;
    for word in [3u32, 0, 3, 6] {
        plain.extend(word.to_le_bytes());
    }
    plain
}

#[test]
fn a_version_four_blob_is_wholly_scrambled_and_keeps_its_first_lod() {
    let baked = decode(&scrambled(&two_lods())).expect("decodes");

    assert_eq!(baked.mesh.indices, [0, 1, 2]);
    assert_eq!(baked.color, Some([200, 10, 10]));
}

#[test]
fn every_version_four_truncation_is_an_error_never_a_panic() {
    let plain = two_lods();
    for len in 0..plain.len() {
        assert!(decode(&scrambled(&plain[..len])).is_err(), "cut at {len}");
    }
}

#[test]
fn a_version_four_lod_count_too_large_for_the_blob_is_truncation() {
    let mut plain = two_lods();
    let at = plain.len() - 16;
    plain[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    assert_eq!(decode(&plain).err(), Some(Error::Truncated));
}

#[test]
fn a_version_four_lod_past_the_indices_is_refused() {
    let mut plain = two_lods();
    // The first LOD's end, 3 → 9, with only 6 indices.
    let at = plain.len() - 8;
    plain[at..at + 4].copy_from_slice(&9u32.to_le_bytes());

    assert_eq!(decode(&plain).err(), Some(Error::Index));
}

/// `RBX_CSGMDL4_FIXTURE=<a raw version 4 MeshData blob>`, such as
/// `meshes/4500696697_4.meshdata` from the `rbx_mesh` crate's repository.
#[test]
#[ignore = "needs RBX_CSGMDL4_FIXTURE (a version 4 MeshData blob)"]
fn a_real_version_four_blob_decodes_its_first_lod_to_a_closed_mesh() {
    let path = std::env::var("RBX_CSGMDL4_FIXTURE").expect("RBX_CSGMDL4_FIXTURE");
    let bytes = std::fs::read(path).expect("fixture must be readable");
    let mesh = decode(&bytes).expect("a real blob decodes").mesh;

    // 1152 indices, of which the first LOD is 1050.
    assert_eq!(mesh.triangle_count(), 350);
    let extent: [f32; 3] = std::array::from_fn(|a| mesh.bounds.max[a] - mesh.bounds.min[a]);
    for (got, want) in extent.iter().zip([118.0, 28.031, 22.034]) {
        assert!((got - want).abs() < 1e-2, "{extent:?}");
    }
    super::v5_tests::assert_closed(&mesh, "version 4");
    // The asset's own tree is an 88 × 28 × 22 wedge and five 0.05-thin wedge
    // slivers off its faces, half their boxes each: 27174.22 in all, and the
    // tree's corners span the extent above. (Both LODs together hold twice
    // that.) `csg` cannot carve it to compare: the slivers leave it leaky.
    let volume = super::v5_tests::volume(&mesh);
    assert!((volume - 27174.22).abs() < 1.0, "{volume}");
}

/// A `PartOperationAsset` holding only a baked mesh, as asset 305197512 does.
fn mesh_only_asset(blob: Vec<u8>) -> Vec<u8> {
    baked_asset(Vec::new(), blob)
}

/// A `PartOperationAsset` holding the tree `child_data` and its bake `blob`.
fn baked_asset(child_data: Vec<u8>, blob: Vec<u8>) -> Vec<u8> {
    let mut dom = WeakDom::new();
    let root = Ref::new(1);
    let mut asset = Instance::new(root, "PartOperationAsset", "Union");
    let properties = asset.properties_mut();
    properties.insert(
        "ChildData".into(),
        Variant::Unknown {
            type_id: 0x01,
            raw: child_data,
        },
    );
    properties.insert(
        "MeshData".into(),
        Variant::Unknown {
            type_id: 0x01,
            raw: blob,
        },
    );
    dom.insert(asset);
    dom.set_parent(root, None);
    rbx_binary::serialize(&dom).expect("synthetic asset must serialize")
}

#[test]
fn a_union_whose_asset_holds_only_a_baked_mesh_draws_that_mesh() {
    let asset_id = "rbxassetid://305197512";
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "UnionOperation", Some(asset_id)));
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let (key, _) = fit(&dom, &database, referent).expect("planned by its AssetId");

    let assets = HashMap::from([(key.clone(), mesh_only_asset(scrambled(&two_triangles())))]);
    let resolution = resolve(
        &plan,
        assets,
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    let mesh = resolution
        .meshes
        .get(&key)
        .expect("the baked mesh is drawn");
    assert_eq!(mesh.indices.len(), 6);
    assert!(resolution.hidden.contains(&referent), "the box is gone");
    assert_eq!(resolution.instances.len(), 1);
}

#[test]
fn a_union_carrying_only_a_baked_mesh_inline_draws_it_with_nothing_to_fetch() {
    let referent = Ref::new(1);
    let mut instance = operation(referent, "UnionOperation", None);
    instance.properties_mut().insert(
        "MeshData2".into(),
        Variant::Unknown {
            type_id: 0x1c,
            raw: scrambled(&two_triangles()),
        },
    );
    let dom = dom_with(instance);
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);

    assert!(plan.assets().is_empty(), "nothing to download");
    let resolution = resolve(
        &plan,
        HashMap::new(),
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );
    let (key, _) = fit(&dom, &database, referent).expect("keyed by its inline mesh");
    assert!(resolution.meshes.contains_key(&key));
    assert!(resolution.hidden.contains(&referent));
}

#[test]
fn a_union_whose_tree_will_not_carve_draws_its_assets_bake() {
    let asset_id = "rbxassetid://4500696697";
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "UnionOperation", Some(asset_id)));
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let (key, _) = fit(&dom, &database, referent).expect("planned by its AssetId");

    let tree = inline_bytes(&fallback_leaves(2));
    let assets = HashMap::from([(key.clone(), baked_asset(tree, scrambled(&two_triangles())))]);
    let resolution = resolve(
        &plan,
        assets,
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    assert_eq!(
        resolution.meshes[&key].indices.len(),
        6,
        "the bake, not pieces"
    );
    assert!(resolution.parts.is_empty());
    assert_eq!(resolution.instances.len(), 1);
}

#[test]
fn a_union_whose_inline_tree_will_not_carve_draws_its_own_bake() {
    let referent = Ref::new(1);
    let mut instance = operation(referent, "UnionOperation", None);
    let properties = instance.properties_mut();
    properties.insert(
        "ChildData2".into(),
        Variant::Unknown {
            type_id: 0x1c,
            raw: inline_bytes(&fallback_leaves(2)),
        },
    );
    properties.insert(
        "MeshData2".into(),
        Variant::Unknown {
            type_id: 0x1c,
            raw: scrambled(&two_triangles()),
        },
    );
    let dom = dom_with(instance);
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let resolution = resolve(
        &plan,
        HashMap::new(),
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    let (key, _) = fit(&dom, &database, referent).expect("keyed by its inline tree");
    assert_eq!(
        resolution.meshes[&key].indices.len(),
        6,
        "the bake, not pieces"
    );
    assert!(resolution.parts.is_empty());
}

#[test]
fn a_union_whose_tree_will_not_carve_and_has_no_bake_draws_its_pieces() {
    let asset_id = "rbxassetid://4500696697";
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "UnionOperation", Some(asset_id)));
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let (key, _) = fit(&dom, &database, referent).expect("planned by its AssetId");

    let assets = HashMap::from([(key.clone(), asset_bytes(&fallback_leaves(2)))]);
    let resolution = resolve(
        &plan,
        assets,
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    assert!(!resolution.meshes.contains_key(&key));
    assert_eq!(resolution.parts.len(), 2);
}

#[test]
fn a_baked_union_without_its_own_colour_takes_the_bakes() {
    let asset_id = "rbxassetid://305197512";
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "UnionOperation", Some(asset_id)));
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let (key, _) = fit(&dom, &database, referent).expect("planned");

    let assets = HashMap::from([(key, mesh_only_asset(two_triangles()))]);
    let resolution = resolve(
        &plan,
        assets,
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    let red = [200.0, 10.0, 10.0].map(|c: f32| super::super::super::srgb_to_linear(c / 255.0));
    assert_eq!(resolution.instances[0].color, red);
}

/// `RBX_CSGMDL_FIXTURE=<a PartOperationAsset .rbxm holding only MeshData>`,
/// such as asset 305197512 from the asset cache.
#[test]
#[ignore = "needs RBX_CSGMDL_FIXTURE (a mesh-only PartOperationAsset)"]
fn a_real_mesh_only_asset_decodes_to_a_closed_mesh_filling_its_bake() {
    let path = std::env::var("RBX_CSGMDL_FIXTURE").expect("RBX_CSGMDL_FIXTURE");
    let bytes = std::fs::read(path).expect("fixture must be readable");
    let baked = of_bytes(&bytes).expect("a real blob decodes");
    let mesh = &baked.mesh;

    assert!(mesh.triangle_count() > 12, "more than a box");
    // Asset 305197512 was baked at an `InitialSize` of 1: its mesh fills
    // the unit box, in the union's own studs.
    for axis in 0..3 {
        assert!((mesh.bounds.max[axis] - mesh.bounds.min[axis] - 1.0).abs() < 1e-3);
    }
    super::v5_tests::assert_closed(mesh, "asset 305197512");
}

/// A union of two primitives made for this test in Studio (a red ball with
/// a blue cylinder taken out), its mesh only in `SolidMeshHolder`, as
/// current Studio saves every union.
const STUDIO_SOLID_MESH: &[u8] =
    include_bytes!("../../../../../../assets/tests/csg/BallMinusCylinder.solidmesh");

#[test]
fn a_studio_solid_mesh_decodes_with_its_biggest_colour() {
    let baked = decode(STUDIO_SOLID_MESH).expect("a SolidMeshHolder decodes");
    assert_eq!(baked.mesh.indices.len(), 560 * 3);
    assert_eq!(baked.color, Some([255, 0, 0]));
    let (min, max) = (baked.mesh.bounds.min, baked.mesh.bounds.max);
    assert!((max[1] - 4.0).abs() < 1e-4 && (min[1] + 4.0).abs() < 1e-4);
    super::v5_tests::assert_closed(&baked.mesh, "BallMinusCylinder");
}

/// An intersection's tree holds its operands, which carving would union:
/// it draws the mesh saved for it instead.
#[test]
fn an_intersection_draws_its_solid_mesh() {
    let referent = Ref::new(1);
    let mut instance = operation(referent, "IntersectOperation", None);
    instance.properties_mut().insert(
        "SolidMeshHolder".into(),
        Variant::Unknown {
            type_id: 0x1c,
            raw: STUDIO_SOLID_MESH.to_vec(),
        },
    );
    let dom = dom_with(instance);
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);
    let resolution = resolve(
        &plan,
        HashMap::new(),
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );
    let (key, _) = fit(&dom, &database, referent).expect("keyed by its inline mesh");
    assert!(resolution.meshes.contains_key(&key));
    assert!(resolution.hidden.contains(&referent));
}
