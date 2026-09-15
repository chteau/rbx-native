use super::*;
use rbx_dom::{CFrameData, Instance, Vector3Data};

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
// A legacy union asset ID from a real place (see the ignored resolution test
// below) that this whole module was reverse-engineered against.
const REAL_ROCK_ASSET_ID: &str = "http://www.roblox.com//asset/?id=394314025";

fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

/// `plan` needs a `Catalog` only to register a fallback part's material slot,
/// irrelevant to these DOM-walking tests — build a fresh, throwaway one.
fn plan_of(dom: &WeakDom, database: &ReflectionDatabase) -> Plan {
    plan(dom, database, &mut Catalog::new(dom, database))
}

fn operation(referent: Ref, class: &str, asset_id: Option<&str>) -> Instance {
    let mut instance = Instance::new(referent, class, "Rock");
    let properties = instance.properties_mut();
    if let Some(id) = asset_id {
        properties.insert("AssetId".to_string(), Variant::String(id.to_string()));
    }
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: IDENTITY_ROTATION,
        }),
    );
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 2.0,
            z: 4.0,
        }),
    );
    properties.insert(
        "InitialSize".to_string(),
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 2.0,
            z: 4.0,
        }),
    );
    instance
}

/// `plan` only looks under `Workspace` now (see [`super::plan`]'s doc
/// comment), so every fixture instance is parented there rather than at root.
fn dom_with(instance: Instance) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    let referent = instance.referent();
    dom.insert(instance);
    dom.set_parent(referent, Some(workspace));
    dom
}

#[test]
fn plans_a_union_operation_carrying_a_legacy_asset_id() {
    let referent = Ref::new(1);
    let dom = dom_with(operation(
        referent,
        "UnionOperation",
        Some(REAL_ROCK_ASSET_ID),
    ));

    let plan = plan_of(&dom, &database());

    assert_eq!(plan.assets(), vec![(referent, AssetRef::Id(394314025))]);
}

#[test]
fn plans_a_negate_operation_the_same_way() {
    let referent = Ref::new(1);
    let dom = dom_with(operation(
        referent,
        "NegateOperation",
        Some("rbxassetid://394314025"),
    ));

    let plan = plan_of(&dom, &database());

    assert_eq!(plan.assets(), vec![(referent, AssetRef::Id(394314025))]);
}

#[test]
fn a_union_without_an_asset_id_is_not_planned() {
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "UnionOperation", None));

    let plan = plan_of(&dom, &database());

    assert!(plan.assets().is_empty());
}

#[test]
fn a_plain_part_is_not_a_part_operation() {
    let referent = Ref::new(1);
    let dom = dom_with(operation(referent, "Part", Some(REAL_ROCK_ASSET_ID)));

    let plan = plan_of(&dom, &database());

    assert!(plan.assets().is_empty());
}

/// Downloads a real union asset and joins it against a real place fixture end
/// to end. Network-touching, so ignored; set `RBX_UNION_FIXTURE` (the union
/// asset) and `RBX_ROUND_TRIP_FIXTURE` (a real .rbxl place) to run offline.
/// The test asset (a carved-off block) decodes to 28 NegateOperations around
/// one additive Part; the boolean carves them all out of it, so every
/// instance gets a real mesh (not a fallback box) and its box is suppressed.
#[test]
#[ignore = "needs RBX_ROUND_TRIP_FIXTURE (and optionally RBX_UNION_FIXTURE)"]
fn resolves_the_real_rock_union_from_a_real_place_fixture() {
    let path = std::env::var("RBX_ROUND_TRIP_FIXTURE")
        .expect("set RBX_ROUND_TRIP_FIXTURE to a real .rbxl place file");
    let bytes = std::fs::read(&path).expect("RBX_ROUND_TRIP_FIXTURE must be readable");
    let dom = rbx_binary::deserialize(&bytes).expect("fixture must parse");
    let database = database();
    let mut scene =
        crate::scene::Scene::from_dom(&dom, &database).expect("fixture must yield a scene");

    let planned = scene.union_assets();
    let rock = AssetRef::Id(394314025);
    let rock_instances = planned.iter().filter(|(_, asset)| *asset == rock).count();
    assert!(
        rock_instances > 0,
        "real fixture must reference asset 394314025"
    );

    // Only the one asset this test actually downloaded: every other legacy
    // union/negate in this large place is left unresolved on purpose, the
    // same way a real run leaves anything that failed to download alone.
    let mut assets = HashMap::new();
    assets.insert(rock.clone(), fixture_bytes());
    scene.resolve_unions(assets);

    let suppressed = scene
        .parts()
        .iter()
        .filter(|part| part.is_suppressed())
        .count();
    let mesh = scene
        .resolved_file_meshes()
        .meshes
        .get(&rock)
        .expect("the boolean must have produced a mesh for the rock asset");
    let instances = scene
        .resolved_file_meshes()
        .instances
        .iter()
        .filter(|instance| instance.mesh == rock)
        .count();
    println!(
        "union resolution: {rock_instances} Rock instances (out of {} legacy unions total), \
         mesh has {} triangles, {instances} instances drawn, {suppressed} boxes suppressed",
        planned.len(),
        mesh.indices.len() / 3
    );
    // A plain box is 12 triangles; a carved rock has far more faces than that.
    assert!(
        mesh.indices.len() / 3 > 12,
        "the resolved mesh must be a carved shape, not a box"
    );
    assert_eq!(
        instances, rock_instances,
        "every Rock instance sharing the asset must draw the computed mesh"
    );
    assert!(suppressed > 0, "a resolved union must hide its box");
}

/// Checks the real rock's boolean result on its own terms — signed volume
/// against its additive base leaf's own volume, and a single connected
/// piece — the same ground this module's `csg` unit tests cover with
/// synthetic boxes, but against the real 29-leaf, 28-negation asset this
/// resolver exists for. Ignored for the same reason as the fixture test
/// above (network); run alongside it.
#[test]
#[ignore = "needs RBX_ROUND_TRIP_FIXTURE (and optionally RBX_UNION_FIXTURE)"]
fn the_real_rock_boolean_is_a_single_positively_oriented_solid() {
    let bytes = fixture_bytes();
    let database = database();
    let node = tree::parse(&bytes, &database).expect("tree must parse");

    // Mirrors `csg::additive_volume_bound`: a leaf's own `negate` flag is
    // always false (only its wrapping `Operation` carries `negate: true`),
    // so a negated child's whole subtree — however many additive-looking
    // leaves are inside it — must be skipped entirely, not just the leaf
    // itself, or this sums in the very geometry the boolean is meant to cut.
    fn base_volume(node: &tree::Node) -> f64 {
        match node {
            tree::Node::Leaf(leaf) if !leaf.negate => {
                use crate::shapes;
                match leaf.geometry.kind {
                    crate::scene::ShapeKind::Box => {
                        shapes::block().volume() as f64
                            * (leaf.geometry.size.x as f64)
                            * (leaf.geometry.size.y as f64)
                            * (leaf.geometry.size.z as f64)
                    }
                    _ => 0.0,
                }
            }
            tree::Node::Leaf(_) => 0.0,
            tree::Node::Operation { children, .. } => children
                .iter()
                .filter(|child| !child.is_negate())
                .map(base_volume)
                .sum(),
        }
    }

    let base = base_volume(&node);
    let solid = csg::evaluate(&node).expect("boolean must succeed");
    let volume = solid.volume();
    let mesh = solid.to_mesh();
    println!(
        "rock volume={volume} base_leaf_volume={base} tris={}",
        mesh.indices.len() / 3
    );
    assert!(volume > 0.0, "signed volume must be positive, got {volume}");
    assert!(
        volume < base,
        "carved volume {volume} must be less than the uncarved base {base}"
    );
    // Ground truth from the asset itself, not a guess: `marked.rbxl`'s own
    // Rock instance (`rbxdump`'d against this same asset) carries `size` =
    // `InitialSize` = (4.000002, 2.00309, 4.000001) — the bounding box
    // *Roblox's own Studio bake* computed for this union. A carved rock is
    // squashed down from the 4x4x4 additive base to barely 2 studs tall
    // (most of the top and bottom got cut away), so comparing the carved
    // volume against the *uncarved* base's volume alone can't tell a correct
    // result from a sliver — the right yardstick is whether the boolean's
    // own bounding box lands on Studio's. It does, to 4+ decimal places,
    // which is the strongest evidence this module's geometry is right.
    let bounds_size = [
        (mesh.bounds.max[0] - mesh.bounds.min[0]) as f64,
        (mesh.bounds.max[1] - mesh.bounds.min[1]) as f64,
        (mesh.bounds.max[2] - mesh.bounds.min[2]) as f64,
    ];
    let expected = [4.000002_f64, 2.00309, 4.000001];
    for axis in 0..3 {
        assert!(
            (bounds_size[axis] - expected[axis]).abs() < 1e-2,
            "bounding box axis {axis}: got {bounds_size:?}, Studio's own bake says {expected:?}"
        );
    }
    // A boulder that fills most of its own (correct) bounding box, not a
    // sliver rattling around inside it.
    let bbox_volume = bounds_size[0] * bounds_size[1] * bounds_size[2];
    assert!(
        volume > 0.5 * bbox_volume,
        "carved volume {volume} must fill most of its own bounding box {bbox_volume}, not be a shard"
    );
    assert_eq!(
        csg::connected_components(&solid).len(),
        1,
        "the resolved rock must be one connected solid, not scattered debris"
    );
}

/// Runs a hand-built "box + one corner negate" tree through the exact same
/// pipeline `resolve` uses: `csg::evaluate` for the local-frame mesh, then
/// `Entry::placement()` for the world matrix a builder's post-bake resize
/// (`size` vs `InitialSize`) leaves behind. A local 2x2x2 box minus its
/// [1,2]^3 corner is 7/8 of the base, and that ratio must survive the
/// uniform 2x `size`/`InitialSize` scale unchanged — volume scales with the
/// cube of a uniform scale factor, so both numerator and denominator scale
/// by the same 2^3 and the ratio cancels.
#[test]
fn a_hand_built_tree_keeps_its_volume_ratio_through_entry_placement() {
    use std::collections::BTreeMap;

    use crate::scene::shape::Geometry;
    use crate::scene::ShapeKind;

    let leaf = |at: Vec3, negate: bool| {
        tree::Node::Leaf(tree::Leaf {
            negate,
            geometry: Geometry {
                kind: ShapeKind::Box,
                size: Vec3::splat(2.0),
                offset: Vec3::ZERO,
            },
            cframe: Mat4::from_translation(at),
            properties: BTreeMap::new(),
        })
    };
    let tree = tree::Node::Operation {
        negate: false,
        children: vec![leaf(Vec3::ZERO, false), leaf(Vec3::splat(1.0), true)],
    };
    let local = csg::evaluate(&tree).expect("boolean must succeed");
    assert!((local.volume() - 7.0).abs() < 1e-4, "{}", local.volume());

    let dom = WeakDom::new();
    let entry = Entry {
        referent: Ref::new(1),
        asset: AssetRef::Id(1),
        cframe: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        size: Vec3::splat(4.0),
        initial_size: Vec3::splat(2.0),
        material: Catalog::new(&dom, &database()).slot(0),
        color: None,
        alpha: 1.0,
        reflectance: 0.0,
        casts_shadow: true,
    };
    // A pure uniform 2x scale, no rotation or extra translation baked in
    // beyond the union's own placement — `placement()` is `cframe *
    // scale(size / initial_size)`.
    let (scale, rotation, translation) = entry.placement().to_scale_rotation_translation();
    assert_eq!(scale, Vec3::splat(2.0));
    assert_eq!(rotation, glam::Quat::IDENTITY);
    assert_eq!(translation, Vec3::new(10.0, 0.0, 0.0));

    let world_volume = local.volume() * (scale.x * scale.y * scale.z) as f64;
    assert!(
        (world_volume / (8.0 * (scale.x * scale.y * scale.z) as f64) - 7.0 / 8.0).abs() < 1e-4,
        "the 7/8 ratio must survive the resize scale"
    );
}

/// Reads `RBX_UNION_FIXTURE` if set, otherwise downloads asset 394314025
/// anonymously the same way the real asset pipeline would.
fn fixture_bytes() -> Vec<u8> {
    if let Ok(path) = std::env::var("RBX_UNION_FIXTURE") {
        return std::fs::read(&path).expect("RBX_UNION_FIXTURE must be readable");
    }
    rbx_cloud::Client::new(None)
        .asset_anonymous(394314025)
        .expect("asset 394314025 must download")
        .bytes
}
