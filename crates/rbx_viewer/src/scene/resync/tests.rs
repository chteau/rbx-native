//! Unit tests for [`super`]: which DOM changes to a part stay a patch of
//! that one part, what each becomes, and which few still need a rebuild.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{in_workspace, Drawn, PartSync};
use crate::changes::Rebuild;
use crate::scene::union::tests_support as union_tests_support;
use crate::scene::{srgb_to_linear, Part, Scene, ShapeKind, UnionEvaluations};

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const WORKSPACE: u32 = 100;
const STORAGE: u32 = 101;
const MODEL_A: u32 = 102;
const MODEL_B: u32 = 103;
const MESH_PART: u32 = 1;
const PLAIN_PART: u32 = 2;
const STAGED: u32 = 3;
const UNION: u32 = 4;
const UNION_ASSET: u64 = 42;
/// How many additive leaves the fixture union's asset carries, and so how
/// many pieces it is drawn as.
const UNION_PIECES: usize = 3;

fn cframe_at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: IDENTITY_ROTATION,
    })
}

fn vector3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn fake_mesh() -> rbx_mesh::Mesh {
    rbx_mesh::Mesh {
        version: (4, 1),
        vertices: Vec::new(),
        indices: Vec::new(),
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-0.5; 3],
            max: [0.5; 3],
        },
    }
}

fn sized(referent: u32, class: &str, x: f32) -> Instance {
    let mut instance = Instance::new(Ref::new(referent), class, class);
    let properties = instance.properties_mut();
    properties.insert("size".to_string(), vector3(2.0, 2.0, 2.0));
    properties.insert("CFrame".to_string(), cframe_at(x, 0.0, 0.0));
    properties.insert(
        "Color3uint8".to_string(),
        Variant::Color3uint8 { r: 0, g: 0, b: 255 },
    );
    instance
}

/// `Workspace { ModelA { MeshPart (mesh downloaded), Part }, ModelB }` next
/// to `ServerStorage { Part(STAGED) }`.
fn place() -> (WeakDom, Scene) {
    let mut dom = WeakDom::new();
    for (referent, class) in [
        (WORKSPACE, "Workspace"),
        (STORAGE, "ServerStorage"),
        (MODEL_A, "Model"),
        (MODEL_B, "Model"),
    ] {
        dom.insert(Instance::new(Ref::new(referent), class, class));
    }
    let mut mesh_part = sized(MESH_PART, "MeshPart", 0.0);
    mesh_part.properties_mut().insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    dom.insert(mesh_part);
    dom.insert(sized(PLAIN_PART, "Part", 4.0));
    dom.insert(sized(STAGED, "Part", 40.0));
    dom.set_parent(Ref::new(WORKSPACE), None);
    dom.set_parent(Ref::new(STORAGE), None);
    dom.set_parent(Ref::new(MODEL_A), Some(Ref::new(WORKSPACE)));
    dom.set_parent(Ref::new(MODEL_B), Some(Ref::new(WORKSPACE)));
    dom.set_parent(Ref::new(MESH_PART), Some(Ref::new(MODEL_A)));
    dom.set_parent(Ref::new(PLAIN_PART), Some(Ref::new(MODEL_A)));
    dom.set_parent(Ref::new(STAGED), Some(Ref::new(STORAGE)));

    let database = ReflectionDatabase::embedded();
    let mut scene = Scene::from_dom(&dom, &database).unwrap();
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh()));
    scene.resolve_file_meshes(meshes, HashMap::new());
    assert_eq!(scene.parts().len(), 2);
    assert_eq!(scene.resolved_file_meshes().instances.len(), 1);
    (dom, scene)
}

fn resync(scene: &mut Scene, dom: &WeakDom, referent: u32) -> Result<PartSync, Rebuild> {
    resync_with(scene, dom, referent, &UnionEvaluations::default())
}

fn resync_with(
    scene: &mut Scene,
    dom: &WeakDom,
    referent: u32,
    unions: &UnionEvaluations,
) -> Result<PartSync, Rebuild> {
    let database = ReflectionDatabase::embedded();
    let known = scene.materials().layers();
    scene.resync_part(dom, &database, Ref::new(referent), known, unions)
}

fn boxed(scene: &mut Scene, dom: &WeakDom, referent: u32) -> Part {
    match resync(scene, dom, referent).map(|sync| sync.drawn) {
        Ok(Drawn::Box(part)) => part,
        other => panic!("expected a box, got {other:?}"),
    }
}

fn meshed(scene: &mut Scene, dom: &WeakDom, referent: u32) -> usize {
    match resync(scene, dom, referent).map(|sync| sync.drawn) {
        Ok(Drawn::Mesh(index)) => index,
        other => panic!("expected a mesh instance, got {other:?}"),
    }
}

/// Asserts `referent` is gone from the picture, with `dropped` piece slots
/// going with it.
fn assert_gone(sync: Result<PartSync, Rebuild>, dropped: std::ops::Range<u32>) {
    let sync = sync.expect("a removal never refuses");
    assert!(
        matches!(sync.drawn, Drawn::Gone),
        "expected nothing drawn, got {:?}",
        sync.drawn
    );
    assert_eq!(sync.dropped, dropped);
}

#[test]
fn a_colour_edit_on_a_box_is_rewritten_in_its_own_slot() {
    let (mut dom, mut scene) = place();
    let slot = scene
        .parts()
        .iter()
        .position(|part| part.referent() == Ref::new(PLAIN_PART))
        .unwrap();

    dom.set_property(
        Ref::new(PLAIN_PART),
        "Color3uint8",
        Variant::Color3uint8 { r: 9, g: 9, b: 9 },
    )
    .unwrap();
    let part = boxed(&mut scene, &dom, PLAIN_PART);

    assert_eq!(
        part.color,
        [9u8, 9, 9].map(|channel| srgb_to_linear(f32::from(channel) / 255.0))
    );
    assert_eq!(scene.parts().len(), 2);
    assert_eq!(scene.parts()[slot].referent(), Ref::new(PLAIN_PART));
}

// Crossing between the opaque, blended and invisible buckets, flipping the
// shadow flag, or changing shape: each is a different GPU batch, and moving
// the record is the renderer's business — the scene still has exactly one
// part to hand it, whatever bucket it belongs in now.
#[test]
fn a_box_keeps_its_place_across_every_bucket_and_shape() {
    let (mut dom, mut scene) = place();
    let part = Ref::new(PLAIN_PART);

    dom.set_property(part, "Transparency", Variant::Float32(0.5))
        .unwrap();
    let boxed_part = boxed(&mut scene, &dom, PLAIN_PART);
    assert!(boxed_part.is_drawn() && boxed_part.is_translucent());

    dom.set_property(part, "Transparency", Variant::Float32(1.0))
        .unwrap();
    assert!(!boxed(&mut scene, &dom, PLAIN_PART).is_drawn());
    // Still placed: a `Decal` on it keeps showing.
    assert!(scene.placement_of(part).is_some());

    dom.set_property(part, "Transparency", Variant::Float32(0.0))
        .unwrap();
    dom.set_property(part, "CastShadow", Variant::Bool(false))
        .unwrap();
    let boxed_part = boxed(&mut scene, &dom, PLAIN_PART);
    assert!(boxed_part.is_drawn() && !boxed_part.casts_shadow());

    // Enum.PartType.Ball
    dom.set_property(part, "shape", Variant::Enum(0)).unwrap();
    assert_eq!(boxed(&mut scene, &dom, PLAIN_PART).kind, ShapeKind::Ball);
    assert_eq!(scene.parts().len(), 2);
}

#[test]
fn a_part_the_scene_never_built_is_added() {
    let (mut dom, mut scene) = place();
    let new = Ref::new(50);
    dom.insert(sized(50, "Part", 200.0));
    dom.set_parent(new, Some(Ref::new(MODEL_B)));
    let before = *scene.bounds();

    let part = boxed(&mut scene, &dom, 50);

    assert_eq!(part.referent(), new);
    assert_eq!(scene.parts().len(), 3);
    assert!(
        scene.refresh_bounds(),
        "a part far outside the extent grows it"
    );
    assert!(scene.bounds().max.x > before.max.x);
}

#[test]
fn a_part_gone_from_the_dom_is_taken_out() {
    let (mut dom, mut scene) = place();
    dom.remove(Ref::new(PLAIN_PART));

    assert_gone(resync(&mut scene, &dom, PLAIN_PART), 0..0);
    assert_eq!(scene.parts().len(), 1);
    assert!(scene.placement_of(Ref::new(PLAIN_PART)).is_none());
    // Nothing to take out a second time.
    assert_gone(resync(&mut scene, &dom, PLAIN_PART), 0..0);
}

// A part's `CFrame` is world-space, so a move between two `Workspace`
// models changes nothing visible — but the part is re-read all the same,
// which is what makes the answer right without a special case.
#[test]
fn a_move_within_workspace_keeps_the_part_and_a_move_out_removes_it() {
    let (mut dom, mut scene) = place();
    let part = Ref::new(PLAIN_PART);

    dom.set_parent(part, Some(Ref::new(MODEL_B)));
    assert_eq!(boxed(&mut scene, &dom, PLAIN_PART).referent(), part);
    assert_eq!(scene.parts().len(), 2);

    dom.set_parent(part, Some(Ref::new(STORAGE)));
    assert_gone(resync(&mut scene, &dom, PLAIN_PART), 0..0);
    assert_eq!(scene.parts().len(), 1);
}

#[test]
fn a_part_moved_into_workspace_is_built() {
    let (mut dom, mut scene) = place();
    let staged = Ref::new(STAGED);
    assert!(!in_workspace(&dom, &ReflectionDatabase::embedded(), staged));

    dom.set_parent(staged, Some(Ref::new(MODEL_B)));

    assert!(in_workspace(&dom, &ReflectionDatabase::embedded(), staged));
    assert_eq!(boxed(&mut scene, &dom, STAGED).referent(), staged);
    assert_eq!(scene.parts().len(), 3);
}

#[test]
fn workspace_itself_and_a_root_are_not_inside_workspace() {
    let (dom, _) = place();
    let database = ReflectionDatabase::embedded();
    assert!(!in_workspace(&dom, &database, Ref::new(WORKSPACE)));
    assert!(!in_workspace(&dom, &database, Ref::new(STORAGE)));
    assert!(in_workspace(&dom, &database, Ref::new(MODEL_A)));
}

// The mesh path: the box is suppressed, so what carries an edit is the
// resolved instance, rewritten where it was.
#[test]
fn a_mesh_parts_edits_land_on_its_resolved_instance() {
    let (mut dom, mut scene) = place();
    let part = Ref::new(MESH_PART);

    dom.set_property(
        part,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let index = meshed(&mut scene, &dom, MESH_PART);
    let instance = &scene.resolved_file_meshes().instances[index];
    assert_eq!(instance.referent, part);
    assert_eq!(instance.color, [srgb_to_linear(1.0), 0.0, 0.0]);

    dom.set_property(part, "CFrame", cframe_at(5.0, 6.0, 7.0))
        .unwrap();
    let index = meshed(&mut scene, &dom, MESH_PART);
    let model = scene.resolved_file_meshes().instances[index].model;
    assert!(model
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(5.0, 6.0, 7.0), 1e-5));

    dom.set_property(part, "Transparency", Variant::Float32(0.5))
        .unwrap();
    dom.set_property(part, "CastShadow", Variant::Bool(false))
        .unwrap();
    let index = meshed(&mut scene, &dom, MESH_PART);
    let instance = &scene.resolved_file_meshes().instances[index];
    assert!((instance.alpha - 0.5).abs() < 1e-6);
    assert!(!instance.casts_shadow);
    assert_eq!(scene.resolved_file_meshes().instances.len(), 1);
    assert!(
        scene.placement_of(part).is_none(),
        "the box stays suppressed under the mesh"
    );
}

// A fully transparent mesh is dropped from the resolved set (a full build
// never lists one) and comes back once visible again — its suppressed box
// is what still says the referent belongs to the mesh path.
#[test]
fn an_invisible_mesh_part_is_gone_and_comes_back() {
    let (mut dom, mut scene) = place();
    let part = Ref::new(MESH_PART);

    dom.set_property(part, "Transparency", Variant::Float32(1.0))
        .unwrap();
    assert_gone(resync(&mut scene, &dom, MESH_PART), 0..0);
    assert!(scene.resolved_file_meshes().instances.is_empty());
    assert_eq!(scene.parts().len(), 2, "the suppressed box is kept");

    dom.set_property(part, "Transparency", Variant::Float32(0.0))
        .unwrap();
    let index = meshed(&mut scene, &dom, MESH_PART);
    assert_eq!(scene.resolved_file_meshes().instances[index].alpha, 1.0);
}

// Swapping to a mesh another instance already draws through is a batch
// move; swapping to one that never downloaded is a download, i.e. a reload.
#[test]
fn a_mesh_id_swap_follows_only_a_downloaded_mesh() {
    let (mut dom, mut scene) = place();
    dom.set_property(
        Ref::new(MESH_PART),
        "MeshId",
        Variant::String("rbxassetid://2".to_string()),
    )
    .unwrap();

    assert_eq!(
        resync(&mut scene, &dom, MESH_PART).err(),
        Some(Rebuild::Asset)
    );

    scene
        .resolved_file_meshes
        .meshes
        .insert(AssetRef::Id(2), Arc::new(fake_mesh()));
    let index = meshed(&mut scene, &dom, MESH_PART);
    assert_eq!(
        scene.resolved_file_meshes().instances[index].mesh,
        AssetRef::Id(2)
    );
}

// The two directions a part can cross between the box and mesh paths:
// emptying a `MeshId` puts the box back; a `MeshPart` inserted fresh whose
// mesh is already resident goes straight to the mesh path.
#[test]
fn a_part_crosses_between_the_box_and_the_mesh_path() {
    let (mut dom, mut scene) = place();
    let part = Ref::new(MESH_PART);

    dom.set_property(part, "MeshId", Variant::String(String::new()))
        .unwrap();
    let boxed_part = boxed(&mut scene, &dom, MESH_PART);
    assert!(boxed_part.is_drawn());
    assert!(scene.resolved_file_meshes().instances.is_empty());
    assert!(scene.placement_of(part).is_some());

    let mut inserted = sized(60, "MeshPart", 8.0);
    inserted.properties_mut().insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://1".to_string()),
    );
    dom.insert(inserted);
    dom.set_parent(Ref::new(60), Some(Ref::new(MODEL_B)));
    let index = meshed(&mut scene, &dom, 60);
    assert_eq!(
        scene.resolved_file_meshes().instances[index].referent,
        Ref::new(60)
    );
    assert_eq!(scene.parts().len(), 3);
    assert!(scene.placement_of(Ref::new(60)).is_none());
}

// A material never seen elsewhere in the place needs a texture-array layer
// the renderer has not uploaded — what a full reload uploads.
#[test]
fn a_material_needing_a_new_layer_is_a_rebuild() {
    let (mut dom, mut scene) = place();
    let database = ReflectionDatabase::embedded();
    let wood = database
        .enum_items("Material")
        .unwrap()
        .iter()
        .find(|(name, _)| name == "Wood")
        .expect("Material enum must carry Wood")
        .1;

    dom.set_property(Ref::new(PLAIN_PART), "Material", Variant::Enum(wood))
        .unwrap();

    assert_eq!(
        resync(&mut scene, &dom, PLAIN_PART).err(),
        Some(Rebuild::Asset)
    );
}

/// `place()` with a `UnionOperation` added, resolved through the very
/// `resolve_unions` a load runs against a synthetic asset whose boolean
/// carves everything away — so the union is drawn as the three additive
/// pieces recovered from its tree, and the evaluations handed back are the
/// ones a real place's `Resident` would be holding.
fn union_place() -> (WeakDom, Scene, UnionEvaluations) {
    let (mut dom, _) = place();
    let mut union = sized(UNION, "UnionOperation", 12.0);
    let properties = union.properties_mut();
    properties.insert(
        "AssetId".to_string(),
        Variant::String(format!("rbxassetid://{UNION_ASSET}")),
    );
    properties.insert("InitialSize".to_string(), vector3(2.0, 2.0, 2.0));
    dom.insert(union);
    dom.set_parent(Ref::new(UNION), Some(Ref::new(MODEL_B)));

    let database = ReflectionDatabase::embedded();
    let mut scene = Scene::from_dom(&dom, &database).unwrap();
    let mut meshes = HashMap::new();
    meshes.insert(AssetRef::Id(1), Arc::new(fake_mesh()));
    scene.resolve_file_meshes(meshes, HashMap::new());

    let mut evaluations = UnionEvaluations::default();
    let mut assets = HashMap::new();
    assets.insert(
        AssetRef::Id(UNION_ASSET),
        union_tests_support::asset_bytes(&union_tests_support::fallback_leaves(UNION_PIECES)),
    );
    scene.resolve_unions(assets, &mut evaluations);

    assert_eq!(
        pieces_of(&scene, UNION).len(),
        UNION_PIECES,
        "the fixture asset must carve to nothing, leaving its additive leaves"
    );
    (dom, scene, evaluations)
}

/// The union's recovered pieces as the scene holds them, in piece order.
fn pieces_of(scene: &Scene, referent: u32) -> Vec<Part> {
    let mut pieces: Vec<Part> = scene
        .parts()
        .iter()
        .filter(|part| part.referent() == Ref::new(referent) && !part.id.is_whole())
        .copied()
        .collect();
    pieces.sort_by_key(|piece| piece.id);
    pieces
}

fn resync_union(scene: &mut Scene, dom: &WeakDom, unions: &UnionEvaluations) -> PartSync {
    resync_with(scene, dom, UNION, unions).expect("a union drawn as its pieces is patched")
}

fn drawn_pieces(sync: &PartSync) -> &[Part] {
    match &sync.drawn {
        Drawn::Pieces { pieces, .. } => pieces,
        other => panic!("expected recovered pieces, got {other:?}"),
    }
}

// The whole point: a union drawn as its recovered pieces is patched piece by
// piece, each in the slot it already had, instead of refusing and rebuilding
// the scene. Nothing is renumbered by the move, so every piece's record is
// the record that piece already occupied.
#[test]
fn a_union_drawn_as_pieces_moves_piece_by_piece() {
    let (mut dom, mut scene, unions) = union_place();
    let before = pieces_of(&scene, UNION);
    let parts = scene.parts().len();

    dom.set_property(Ref::new(UNION), "CFrame", cframe_at(12.0, 30.0, 0.0))
        .unwrap();
    let sync = resync_union(&mut scene, &dom, &unions);

    let drawn = drawn_pieces(&sync);
    assert_eq!(drawn.len(), UNION_PIECES);
    assert!(sync.dropped.is_empty(), "no piece slot was given up");
    let after = pieces_of(&scene, UNION);
    assert_eq!(
        after.iter().map(|piece| piece.id).collect::<Vec<_>>(),
        before.iter().map(|piece| piece.id).collect::<Vec<_>>(),
        "a move renumbers nothing"
    );
    assert_eq!(scene.parts().len(), parts, "no piece was added or dropped");
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(before.color, after.color, "a piece keeps its own colour");
        let shift = after.transform.transform_point3(Vec3::ZERO)
            - before.transform.transform_point3(Vec3::ZERO);
        assert!(
            shift.abs_diff_eq(Vec3::new(0.0, 30.0, 0.0), 1e-4),
            "{shift}"
        );
    }
}

// The union itself is what the Explorer, an outline, a decal and a click all
// address, so the one placement the scene keeps for it is its own box — the
// very box `pick` hit-tests it as — and not one of the pieces standing
// inside that box.
#[test]
fn a_union_drawn_as_pieces_is_placed_by_its_own_box() {
    let (mut dom, mut scene, unions) = union_place();
    let union = Ref::new(UNION);

    let placement = scene.placement_of(union).expect("the union's own box");
    assert!(placement
        .model
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(12.0, 0.0, 0.0), 1e-5));
    assert_eq!(scene.placements().get(&union), Some(&placement));

    dom.set_property(Ref::new(UNION), "CFrame", cframe_at(12.0, 30.0, 0.0))
        .unwrap();
    let sync = resync_union(&mut scene, &dom, &unions);

    let Drawn::Pieces { placement, .. } = &sync.drawn else {
        panic!("expected recovered pieces");
    };
    assert!(placement
        .model
        .transform_point3(Vec3::ZERO)
        .abs_diff_eq(Vec3::new(12.0, 30.0, 0.0), 1e-5));
    assert_eq!(scene.placement_of(union).as_ref(), Some(placement));
}

// A union's own colour and transparency belong to the mesh its boolean would
// have produced; the pieces carry the colour and transparency of the parts
// they were recovered from, which is what a rebuild draws too. The edit still
// has to be patched rather than rebuilt for.
#[test]
fn recolouring_a_union_drawn_as_pieces_leaves_its_pieces_alone() {
    let (mut dom, mut scene, unions) = union_place();
    let before = pieces_of(&scene, UNION);

    dom.set_property(
        Ref::new(UNION),
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    dom.set_property(Ref::new(UNION), "Transparency", Variant::Float32(1.0))
        .unwrap();
    let sync = resync_union(&mut scene, &dom, &unions);

    let drawn = drawn_pieces(&sync);
    assert_eq!(drawn.len(), UNION_PIECES);
    assert!(drawn.iter().all(Part::is_drawn));
    assert_eq!(
        pieces_of(&scene, UNION)
            .iter()
            .map(|piece| piece.color)
            .collect::<Vec<_>>(),
        before.iter().map(|piece| piece.color).collect::<Vec<_>>()
    );
}

// A deleted union takes every piece with it, and the renderer is told which
// piece slots to let go of — nothing keys them but the union's own referent.
#[test]
fn a_union_deleted_goes_with_its_pieces() {
    let (mut dom, mut scene, unions) = union_place();
    let parts = scene.parts().len();
    dom.remove(Ref::new(UNION));

    assert_gone(
        resync_with(&mut scene, &dom, UNION, &unions),
        0..UNION_PIECES as u32,
    );

    assert_eq!(scene.parts().len(), parts - UNION_PIECES - 1);
    assert!(pieces_of(&scene, UNION).is_empty());
    assert!(scene.placement_of(Ref::new(UNION)).is_none());
}

// An operation tree with nothing additive left in it recovers no piece at
// all: `resolve_unions` hides the box regardless (an empty union being a
// better guess than a solid one), so there is simply nothing to draw — and
// an edit to it is still an edit, not a rebuild.
#[test]
fn a_union_with_no_recovered_pieces_draws_nothing() {
    let (mut dom, _, _) = union_place();
    let database = ReflectionDatabase::embedded();
    let mut scene = Scene::from_dom(&dom, &database).unwrap();
    let mut evaluations = UnionEvaluations::default();
    let mut assets = HashMap::new();
    assets.insert(
        AssetRef::Id(UNION_ASSET),
        union_tests_support::asset_bytes(&[union_tests_support::Leaf::negation(Vec3::ZERO, 2.0)]),
    );
    scene.resolve_unions(assets, &mut evaluations);
    assert!(pieces_of(&scene, UNION).is_empty());

    dom.set_property(Ref::new(UNION), "CFrame", cframe_at(12.0, 30.0, 0.0))
        .unwrap();
    let sync = resync_with(&mut scene, &dom, UNION, &evaluations).expect("still an edit");

    assert!(drawn_pieces(&sync).is_empty());
    assert!(sync.dropped.is_empty());
    assert!(
        scene.placement_of(Ref::new(UNION)).is_none(),
        "a union that draws nothing places nothing either"
    );
}

// A union whose asset this place never carved — nothing downloaded it, or
// its bytes would not parse — draws as its own box, exactly as a rebuild
// leaves it. Only an edit pointing it at an asset nobody has fetched needs
// the load-time path.
#[test]
fn a_union_whose_asset_was_never_carved_stays_a_box() {
    let (mut dom, mut scene, _) = union_place();
    let none = UnionEvaluations::default();

    let sync = resync_with(&mut scene, &dom, UNION, &none).expect("the box is still patchable");

    let Drawn::Box(part) = &sync.drawn else {
        panic!("expected the union's own box, got {:?}", sync.drawn);
    };
    assert!(part.is_drawn());
    assert_eq!(sync.dropped, 0..UNION_PIECES as u32);
    assert!(pieces_of(&scene, UNION).is_empty());

    dom.set_property(
        Ref::new(UNION),
        "AssetId",
        Variant::String("rbxassetid://999".to_string()),
    )
    .unwrap();
    assert_eq!(
        resync_with(&mut scene, &dom, UNION, &none).err(),
        Some(Rebuild::Asset),
        "an asset nobody has fetched is a load's to carve"
    );
}

// The extent a rebuild would compute: every instance's own box as the DOM
// lists it, but not a failed union's recovered pieces, which `Scene::from_dom`
// never had in hand when it took its own.
#[test]
fn bounds_leave_out_a_unions_recovered_pieces() {
    let (mut dom, mut scene, unions) = union_place();
    let reach = |scene: &Scene| {
        pieces_of(scene, UNION)
            .iter()
            .map(|piece| piece.transform.transform_point3(Vec3::ZERO).x)
            .fold(f32::MIN, f32::max)
    };
    assert!(
        reach(&scene) > scene.bounds().max.x,
        "the fixture's pieces must reach past the union's own box"
    );

    dom.set_property(Ref::new(UNION), "CFrame", cframe_at(100.0, 0.0, 0.0))
        .unwrap();
    resync_union(&mut scene, &dom, &unions);

    assert!(
        scene.refresh_bounds(),
        "the union's own box moved the extent"
    );
    assert!(
        (scene.bounds().max.x - 101.0).abs() < 1e-4,
        "the extent must stop at the union's own box, not its pieces: {:?}",
        scene.bounds()
    );
}
