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
    scene.resolve_unions(assets, &mut Evaluations::default());

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

/// One additive or negated box leaf of a hand-built operation tree, `at`
/// studs along X in the union's own frame, painted `color` where it has one.
fn test_leaf(at: f32, negate: bool, color: Option<[u8; 3]>) -> tree::Node {
    use std::collections::BTreeMap;

    use crate::scene::shape::Geometry;
    use crate::scene::ShapeKind;

    let mut properties = BTreeMap::new();
    if let Some([r, g, b]) = color {
        properties.insert("Color3uint8".to_string(), Variant::Color3uint8 { r, g, b });
    }
    tree::Node::Leaf(tree::Leaf {
        negate,
        geometry: Geometry {
            kind: ShapeKind::Box,
            size: Vec3::splat(2.0),
            offset: Vec3::ZERO,
        },
        cframe: Mat4::from_translation(Vec3::new(at, 0.0, 0.0)),
        properties,
    })
}

/// A carving as `Evaluations` holds one: the tree, plus the computed mesh
/// only where the boolean is meant to have succeeded — which is exactly what
/// tells a union drawn as one mesh from one drawn as its pieces.
fn carving(children: Vec<tree::Node>, carved: bool) -> Evaluated {
    let tree = tree::Node::Operation {
        negate: false,
        children,
    };
    let mesh = carved.then(|| {
        Arc::new(
            csg::evaluate(&tree)
                .expect("the boolean must run")
                .to_mesh(),
        )
    });
    Evaluated { tree, mesh }
}

/// The union fixture `replan` reads, `UsePartColor` on and painted red.
fn union_dom() -> WeakDom {
    let mut instance = operation(Ref::new(1), "UnionOperation", Some("rbxassetid://42"));
    instance.properties_mut().insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    );
    instance
        .properties_mut()
        .insert("UsePartColor".to_string(), Variant::Bool(true));
    dom_with(instance)
}

// `Scene::resync_part` re-plans a union through `replan`/`patched` against
// the boolean this place already carved. That carving is what lets a colour
// coming from the pieces (`UsePartColor` off) be repainted here too — it is
// read off the very tree `resolve` reads it off.
#[test]
fn a_replanned_union_takes_its_colour_from_itself_or_from_its_tree() {
    let database = database();
    let mut dom = union_dom();
    let mut materials = Catalog::new(&dom, &database);
    let carved = carving(vec![test_leaf(0.0, false, Some([0, 0, 255]))], true);
    let mut resolved = crate::scene::Resolved::default();
    resolved.meshes.insert(
        AssetRef::Id(42),
        carved.mesh.clone().expect("the boolean succeeded"),
    );

    let entry = replan(&dom, &database, Ref::new(1), &mut materials).expect("a union re-plans");
    let patched = entry
        .patched(&carved, &resolved)
        .expect("UsePartColor on: the union's own colour");
    assert_eq!(patched.referent, Ref::new(1));
    assert_eq!(patched.mesh, AssetRef::Id(42));
    assert_eq!(patched.color, [super::super::srgb_to_linear(1.0), 0.0, 0.0]);
    assert!(patched
        .model
        .transform_point3(glam::Vec3::ZERO)
        .abs_diff_eq(glam::Vec3::new(1.0, 2.0, 3.0), 1e-5));

    dom.set_property(Ref::new(1), "UsePartColor", Variant::Bool(false))
        .unwrap();
    let entry = replan(&dom, &database, Ref::new(1), &mut materials).unwrap();
    let patched = entry
        .patched(&carved, &resolved)
        .expect("UsePartColor off: the biggest additive leaf's colour");
    assert_eq!(patched.color, [0.0, 0.0, super::super::srgb_to_linear(1.0)]);
}

// A mesh this renderer never uploaded is the one thing a re-planned union
// still cannot draw on its own — an edit that points it at another asset.
#[test]
fn a_union_whose_computed_mesh_is_not_resident_does_not_patch() {
    let database = database();
    let dom = union_dom();
    let mut materials = Catalog::new(&dom, &database);
    let carved = carving(vec![test_leaf(0.0, false, None)], true);

    let entry = replan(&dom, &database, Ref::new(1), &mut materials).expect("a union re-plans");

    assert!(entry
        .patched(&carved, &crate::scene::Resolved::default())
        .is_none());
}

// The piece identity the renderer addresses each fallback piece by: its
// position in the tree's additive order. Re-deriving the same tree at another
// placement — the union moved — has to hand every leaf the same number back,
// or an edit would rewrite the record of a different piece than it moved.
#[test]
fn a_unions_pieces_are_numbered_in_tree_order_and_keep_their_numbers() {
    let database = database();
    let mut dom = union_dom();
    let mut materials = Catalog::new(&dom, &database);
    // The negated leaf sits between two additive ones: it is carved away
    // rather than drawn, and must not consume a number either.
    let carved = carving(
        vec![
            test_leaf(0.0, false, Some([255, 0, 0])),
            test_leaf(1.0, true, None),
            test_leaf(4.0, false, Some([0, 255, 0])),
        ],
        false,
    );
    let entry = replan(&dom, &database, Ref::new(1), &mut materials).expect("a union re-plans");

    let pieces = entry.pieces(&carved, &database, &mut materials);

    let ids: Vec<_> = pieces.iter().map(|piece| piece.id).collect();
    assert_eq!(
        ids,
        vec![
            crate::scene::PartId::piece(Ref::new(1), 0),
            crate::scene::PartId::piece(Ref::new(1), 1),
        ],
        "one number per surviving additive leaf, in tree order"
    );

    // The union moved: every piece follows it, and not one of them is
    // renumbered by the move.
    dom.set_property(
        Ref::new(1),
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 100.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: IDENTITY_ROTATION,
        }),
    )
    .unwrap();
    let moved = replan(&dom, &database, Ref::new(1), &mut materials).expect("a union re-plans");

    let after = moved.pieces(&carved, &database, &mut materials);

    assert_eq!(
        after.iter().map(|piece| piece.id).collect::<Vec<_>>(),
        ids,
        "a move renumbers nothing"
    );
    for (before, after) in pieces.iter().zip(&after) {
        assert_eq!(before.color, after.color);
        let shift = after.transform.transform_point3(Vec3::ZERO)
            - before.transform.transform_point3(Vec3::ZERO);
        // The fixture union stands at (1, 2, 3) and the move put it at
        // (100, 0, 0): every piece has to have travelled exactly that.
        assert!(
            shift.abs_diff_eq(Vec3::new(99.0, -2.0, -3.0), 1e-4),
            "{shift}"
        );
    }
}

// An all-negated tree recovers nothing to draw: the union hides its box
// (`resolve` hides it whatever the tree held) and stands for no piece at all.
#[test]
fn a_union_with_no_additive_leaf_recovers_no_pieces() {
    let database = database();
    let dom = union_dom();
    let mut materials = Catalog::new(&dom, &database);
    let carved = carving(vec![test_leaf(0.0, true, None)], false);
    let entry = replan(&dom, &database, Ref::new(1), &mut materials).expect("a union re-plans");

    assert!(entry.pieces(&carved, &database, &mut materials).is_empty());
}

/// Builds one synthetic legacy union asset's raw bytes: a `PartOperationAsset`
/// root whose `ChildData` is a nested `.rbxm` of one large additive base box
/// and `leaves` small `NegateOperation` boxes scattered through it, the same
/// shape (one additive leaf, many negations) as the real rock asset this
/// module was reverse-engineered against — see `REAL_ROCK_ASSET_ID` above.
/// `seed` scatters the negations differently per call so two calls never
/// serialize to identical bytes (real unions in a place never do either).
///
/// Used both to profile [`resolve`]'s CSG cost against a place with many
/// unions (no network, no committed asset — see `agents/AGENTS.md`'s asset
/// rules) and, via [`synthetic_plan`], to check a parallel evaluation order
/// against this module's sequential one.
fn synthetic_asset_bytes(seed: u32, leaves: usize) -> Vec<u8> {
    let mut nodes = vec![tests_support::Leaf::additive(Vec3::ZERO, 10.0)];
    nodes.extend((0..leaves).map(|i| {
        let t = (seed as f32 * 31.0 + i as f32) * 0.7;
        // Deterministic pseudo-scatter (sin/cos of an index, no `rand`
        // dependency for one throwaway coordinate source) keeping every
        // negation's centre inside the base box so the boolean has real
        // carving to do rather than degenerating into no-op disjoint cuts.
        let center = Vec3::new(t.sin(), (t * 1.3).cos(), (t * 1.7).sin()) * 3.5;
        tests_support::Leaf::negation(center, 1.5)
    }));
    tests_support::asset_bytes(&nodes)
}

/// A [`Plan`] of `unions` entries, each its own distinct synthetic asset of
/// `leaves_per_union` leaves (see [`synthetic_asset_bytes`]), plus the byte
/// map [`resolve`] needs to evaluate every one of them — a synthetic stand-in
/// for a real, CSG-heavy place with many different unions.
fn synthetic_plan(
    unions: usize,
    leaves_per_union: usize,
    materials: &Catalog,
) -> (Plan, HashMap<AssetRef, Vec<u8>>) {
    let mut assets = HashMap::new();
    let mut entries = Vec::new();
    for i in 0..unions {
        let asset = AssetRef::Id(1_000_000 + i as u64);
        assets.insert(
            asset.clone(),
            synthetic_asset_bytes(i as u32, leaves_per_union),
        );
        entries.push(Entry {
            referent: Ref::new(2_000 + i as u32),
            asset,
            cframe: Mat4::IDENTITY,
            size: Vec3::splat(10.0),
            initial_size: Vec3::splat(10.0),
            material: materials.slot(0),
            color: None,
            alpha: 1.0,
            reflectance: 0.0,
            casts_shadow: true,
        });
    }
    (Plan { entries }, assets)
}

/// `resolve` runs every union's BSP boolean across a bounded worker pool
/// (`evaluate_all`) rather than one at a time on the caller's own thread,
/// since each asset's boolean is independent of every other. This checks
/// that the evaluation order never changes the result: each asset's mesh
/// must come out byte-for-byte identical to evaluating it alone — a race or
/// an ordering bug in the parallel path would show up here as a mismatched
/// vertex/index list, not as a crash, which is exactly the kind of silent
/// regression a "just check it's faster" test would miss.
///
/// Run several times over: a race that only sometimes reorders two threads'
/// writes would not necessarily show up on the first call.
#[test]
fn parallel_csg_evaluation_matches_evaluating_each_asset_alone() {
    let database = database();
    let dom = WeakDom::new();
    let mut materials = Catalog::new(&dom, &database);
    // More entries than `MAX_CSG_WORKERS` so every worker thread actually
    // picks up more than one asset — the scenario a single-asset run can't
    // exercise at all.
    let (plan, assets) = synthetic_plan(12, 6, &materials);

    let expected: HashMap<AssetRef, Arc<rbx_mesh::Mesh>> = assets
        .iter()
        .map(|(asset, bytes)| {
            let evaluated = evaluate(bytes, &database).expect("synthetic asset must parse");
            (
                asset.clone(),
                evaluated.mesh.expect("synthetic boolean must succeed"),
            )
        })
        .collect();
    assert_eq!(expected.len(), plan.entries.len());

    for _ in 0..5 {
        let resolution = resolve(
            &plan,
            assets.clone(),
            &database,
            &mut materials,
            &mut Evaluations::default(),
        );
        assert_eq!(resolution.meshes.len(), expected.len());
        for (asset, mesh) in &expected {
            let got = resolution
                .meshes
                .get(asset)
                .expect("every asset evaluated alone must also resolve through the pool");
            assert_eq!(
                got, mesh,
                "asset {asset:?}: the pooled evaluation produced a different mesh than \
                 evaluating the same asset alone"
            );
        }
    }
}

// A reload re-plans every union and resolves the same assets again: with the
// evaluations kept, the second resolve must hand out the very meshes the
// first carved — the same allocation, not an equal one computed again — and
// still resolve every instance exactly as the first did.
#[test]
fn a_second_resolve_reuses_every_boolean_already_carved() {
    let database = database();
    let dom = WeakDom::new();
    let mut materials = Catalog::new(&dom, &database);
    let (plan, assets) = synthetic_plan(4, 3, &materials);
    let mut evaluations = Evaluations::default();

    let first = resolve(
        &plan,
        assets.clone(),
        &database,
        &mut materials,
        &mut evaluations,
    );
    let second = resolve(&plan, assets, &database, &mut materials, &mut evaluations);

    assert_eq!(first.meshes.len(), plan.entries.len());
    assert_eq!(second.instances.len(), first.instances.len());
    assert_eq!(second.hidden, first.hidden);
    for (asset, mesh) in &first.meshes {
        let again = second
            .meshes
            .get(asset)
            .expect("the same asset resolves again");
        assert!(
            Arc::ptr_eq(mesh, again),
            "asset {asset:?}: the boolean was carved a second time"
        );
    }
}

// A reload skips fetching the bytes of every asset already carved (see
// `load::resolve_unions`), so a second resolve with none of them must still
// find every union in the evaluations and hand out the same meshes.
#[test]
fn a_known_asset_resolves_without_its_bytes() {
    let database = database();
    let dom = WeakDom::new();
    let mut materials = Catalog::new(&dom, &database);
    let (plan, assets) = synthetic_plan(3, 3, &materials);
    let mut evaluations = Evaluations::default();

    let first = resolve(&plan, assets, &database, &mut materials, &mut evaluations);
    let second = resolve(
        &plan,
        HashMap::new(),
        &database,
        &mut materials,
        &mut evaluations,
    );

    assert_eq!(first.meshes.len(), plan.entries.len());
    assert_eq!(second.instances.len(), first.instances.len());
    assert_eq!(second.hidden, first.hidden);
    for (asset, mesh) in &first.meshes {
        assert!(evaluations.is_known(asset));
        let again = second.meshes.get(asset).expect("resolved without bytes");
        assert!(Arc::ptr_eq(mesh, again));
    }
}

/// Manual profiling harness, not part of the regular gate: run with
/// `cargo test --release -p rbx_viewer --lib union::tests::profile_synthetic_csg_heavy_place -- --ignored --nocapture`
/// to see where `resolve`'s time actually goes on a synthetic, CSG-heavy
/// place (this repository ships no real one — see `agents/AGENTS.md`'s asset
/// rules). Prints wall time only; a caller comparing before/after times it
/// externally (`/usr/bin/time` or similar) to also see CPU time and thread
/// fan-out.
#[test]
#[ignore = "manual profiling harness, see doc comment"]
fn profile_synthetic_csg_heavy_place() {
    let database = database();
    let dom = WeakDom::new();
    let mut materials = Catalog::new(&dom, &database);
    let (plan, assets) = synthetic_plan(40, 30, &materials);

    let start = std::time::Instant::now();
    let resolution = resolve(
        &plan,
        assets,
        &database,
        &mut materials,
        &mut Evaluations::default(),
    );
    let elapsed = start.elapsed();

    println!(
        "resolve: {elapsed:?} for {} unions, {} meshes computed",
        plan.entries.len(),
        resolution.meshes.len()
    );
    assert!(!resolution.meshes.is_empty());
}
