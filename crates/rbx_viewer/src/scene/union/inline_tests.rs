//! Unions whose tree is not behind their own `AssetId`: carried inline in
//! `ChildData2`, nested operations naming an asset of their own, nested
//! operations resized since their bake, and unions with no tree at all.

use std::collections::HashMap;

use glam::Vec3;
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};

use super::tests::{database, dom_with, operation, plan_of};
use super::tests_support::{asset_bytes, inline_bytes, Leaf};
use super::*;

const SHARED_STRING: u8 = 0x1c;

fn vector(v: Vec3) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v.x,
        y: v.y,
        z: v.z,
    })
}

fn at_origin() -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

/// A 2-stud box with a 1-stud corner carved out of it.
fn notched() -> Vec<Leaf> {
    vec![
        Leaf::additive(Vec3::ZERO, 2.0),
        Leaf::negation(Vec3::splat(1.0), 1.0),
    ]
}

/// A union in `Workspace` carrying `tree` inline, as Studio writes one now.
fn inline_union(tree: Vec<u8>, type_id: u8) -> (WeakDom, Ref) {
    let referent = Ref::new(1);
    let mut instance = operation(referent, "UnionOperation", None);
    instance
        .properties_mut()
        .insert("ChildData2".into(), Variant::Unknown { type_id, raw: tree });
    (dom_with(instance), referent)
}

/// An operation document holding one nested `UnionOperation`, sized `size`
/// against a bake of `initial`, whose tree is either `child_data` or the
/// asset `asset_id` names.
fn nested_operation(
    child_data: Option<Vec<u8>>,
    asset_id: Option<&str>,
    size: Vec3,
    initial: Vec3,
) -> Vec<u8> {
    let mut dom = WeakDom::new();
    let referent = Ref::new(1);
    let mut instance = Instance::new(referent, "UnionOperation", "Inner");
    let properties = instance.properties_mut();
    properties.insert("CFrame".into(), at_origin());
    properties.insert("size".into(), vector(size));
    properties.insert("InitialSize".into(), vector(initial));
    if let Some(raw) = child_data {
        properties.insert("ChildData".into(), Variant::Unknown { type_id: 0x01, raw });
    }
    if let Some(id) = asset_id {
        properties.insert("AssetId".into(), Variant::String(id.into()));
    }
    dom.insert(instance);
    dom.set_parent(referent, None);
    rbx_binary::serialize(&dom).expect("synthetic document must serialize")
}

#[test]
fn an_inline_tree_parses_without_an_asset_around_it() {
    let parsed = tree::parse(&inline_bytes(&notched()), &database(), &tree::Assets::new())
        .expect("the document is the tree itself");

    assert_eq!(parsed.root.leaf_count(), 2);
    assert!(parsed.missing.is_empty());
}

#[test]
fn a_union_carrying_its_tree_is_carved_with_nothing_to_download() {
    let (dom, referent) = inline_union(inline_bytes(&notched()), SHARED_STRING);
    let database = database();
    let mut materials = Catalog::new(&dom, &database);
    let plan = plan(&dom, &database, &mut materials);

    assert!(plan.assets().is_empty(), "an inline tree is never fetched");
    let resolution = resolve(
        &plan,
        HashMap::new(),
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );

    let (key, _) = fit(&dom, &database, referent).expect("an inline union has a key");
    let mesh = resolution
        .meshes
        .get(&key)
        .expect("the export finds the carved mesh under `fit`'s key");
    assert!(mesh.indices.len() / 3 > 12, "carved, not a box");
    assert!(resolution.hidden.contains(&referent));
}

#[test]
fn a_shared_string_read_from_xml_is_recognised_too() {
    // `rbx_xml` reports a non-UTF-8 SharedString under String's type id.
    let (dom, referent) = inline_union(inline_bytes(&notched()), 0x01);

    assert!(fit(&dom, &database(), referent).is_some());
}

#[test]
fn copies_of_one_inline_union_share_one_key() {
    let (dom, referent) = inline_union(inline_bytes(&notched()), SHARED_STRING);
    let (other, other_referent) = inline_union(inline_bytes(&notched()), SHARED_STRING);
    let database = database();

    assert_eq!(
        fit(&dom, &database, referent).map(|(key, _)| key),
        fit(&other, &database, other_referent).map(|(key, _)| key)
    );
}

#[test]
fn a_nested_operation_naming_an_asset_waits_for_it() {
    let document = nested_operation(
        None,
        Some("rbxassetid://77"),
        Vec3::splat(2.0),
        Vec3::splat(2.0),
    );
    let database = database();

    let pending = tree::parse(&document, &database, &tree::Assets::new()).expect("parses");
    assert_eq!(pending.missing, vec![AssetRef::Id(77)]);

    let assets = HashMap::from([(AssetRef::Id(77), asset_bytes(&notched()))]);
    let landed = tree::parse(&document, &database, &assets).expect("parses");
    assert!(landed.missing.is_empty());
    assert_eq!(landed.root.leaf_count(), 2, "the asset's own two leaves");

    // One that failed for good is handed over empty: drawn as its box.
    let failed = HashMap::from([(AssetRef::Id(77), Vec::new())]);
    let boxed = tree::parse(&document, &database, &failed).expect("parses");
    assert!(boxed.missing.is_empty());
    assert_eq!(boxed.root.leaf_count(), 1);
}

#[test]
fn an_inline_tree_names_its_nested_assets_until_carved() {
    let document = nested_operation(
        None,
        Some("rbxassetid://77"),
        Vec3::splat(2.0),
        Vec3::splat(2.0),
    );
    let (dom, _) = inline_union(document, SHARED_STRING);
    let database = database();
    let plan = plan_of(&dom, &database);

    assert!(
        plan.assets().is_empty(),
        "the union itself downloads nothing"
    );
    let mut evaluations = Evaluations::default();
    assert_eq!(
        missing(&plan, &HashMap::new(), &database, &evaluations),
        vec![AssetRef::Id(77)]
    );

    let assets = HashMap::from([(AssetRef::Id(77), asset_bytes(&notched()))]);
    assert!(missing(&plan, &assets, &database, &evaluations).is_empty());
    let mut materials = Catalog::new(&dom, &database);
    resolve(&plan, assets, &database, &mut materials, &mut evaluations);
    assert!(
        missing(&plan, &HashMap::new(), &database, &evaluations).is_empty(),
        "a carved tree asks for nothing more"
    );
}

#[test]
fn a_tree_waiting_on_a_nested_asset_is_not_carved_yet() {
    let document = nested_operation(
        None,
        Some("rbxassetid://77"),
        Vec3::splat(2.0),
        Vec3::splat(2.0),
    );
    let database = database();

    assert!(evaluate(&document, &database, &tree::Assets::new(), None, None).is_none());
    let assets = HashMap::from([(AssetRef::Id(77), asset_bytes(&notched()))]);
    let carved = evaluate(&document, &database, &assets, None, None)
        .flatten()
        .expect("parses");
    assert!(carved.is_carved());
}

#[test]
fn a_nested_operation_resized_since_its_bake_scales_its_tree() {
    // A 2-stud cube baked into a nested union, then stretched to 4 studs.
    let inner = inline_bytes(&[Leaf::additive(Vec3::ZERO, 2.0)]);
    let document = nested_operation(Some(inner), None, Vec3::splat(4.0), Vec3::splat(2.0));

    let parsed = tree::parse(&document, &database(), &tree::Assets::new()).expect("parses");
    let solid = csg::evaluate(&parsed.root, None).expect("a cube carves");

    assert!((solid.volume() - 64.0).abs() < 1e-3, "{}", solid.volume());
}

#[test]
fn a_union_with_no_geometry_anywhere_is_empty() {
    let database = database();
    let bare = dom_with(operation(Ref::new(1), "UnionOperation", None));
    assert!(is_empty(&bare, &database, Ref::new(1)));

    let (inline, referent) = inline_union(inline_bytes(&notched()), SHARED_STRING);
    assert!(!is_empty(&inline, &database, referent));

    let by_asset = dom_with(operation(
        Ref::new(1),
        "UnionOperation",
        Some("rbxassetid://5"),
    ));
    assert!(!is_empty(&by_asset, &database, Ref::new(1)));

    let mut baked = operation(Ref::new(1), "UnionOperation", None);
    baked.properties_mut().insert(
        "MeshData2".into(),
        Variant::Unknown {
            type_id: SHARED_STRING,
            raw: vec![1, 2, 3],
        },
    );
    assert!(!is_empty(&dom_with(baked), &database, Ref::new(1)));

    let part = dom_with(operation(Ref::new(1), "Part", None));
    assert!(
        !is_empty(&part, &database, Ref::new(1)),
        "only a union can be empty"
    );
}

#[test]
fn an_empty_union_draws_nothing() {
    let database = database();
    let dom = dom_with(operation(Ref::new(1), "UnionOperation", None));
    let scene = crate::scene::Scene::from_dom(&dom, &database).expect("a scene");

    let part = scene
        .parts()
        .iter()
        .find(|part| part.referent() == Ref::new(1))
        .expect("still a part, for selection and bounds of its own");
    assert!(part.is_suppressed());
}

#[test]
fn a_tree_with_a_mesh_part_draws_the_unions_own_bake() {
    let database = database();
    let document = inline_bytes(&[Leaf::additive(Vec3::ZERO, 2.0), Leaf::mesh(Vec3::X, 2.0)]);
    let parsed = tree::parse(&document, &database, &tree::Assets::new()).unwrap();
    assert!(parsed.meshes);
    let wedge = rbx_csg::Solid::from_mesh(&rbx_csg::shapes::wedge(), glam::Mat4::IDENTITY);
    let bake = rbx_csg::solidmesh::encode(&wedge);
    let drawn = evaluate(
        &document,
        &database,
        &tree::Assets::new(),
        None,
        Some(&bake),
    )
    .flatten()
    .and_then(|evaluated| evaluated.mesh)
    .expect("the bake draws");
    let baked = baked::of_bytes(&bake).unwrap().mesh.triangle_count();
    assert_eq!(drawn.indices.len() / 3, baked);
}
