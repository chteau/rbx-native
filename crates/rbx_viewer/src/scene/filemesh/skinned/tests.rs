use std::collections::HashMap;

use glam::Vec3;
use rbx_dom::{CFrameData, Instance, Variant, Vector3Data};
use rbx_mesh::{Bone, Envelope, Skin, Subset};

use super::*;
use crate::scene::filemesh::{plan, resolve, Plan, Resolved};
use crate::scene::material::Catalog;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// 90° about +Z, row-major: +X goes to +Y.
const QUARTER_Z: [f32; 9] = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

const MODEL: Ref = Ref::new(2);
const UPPER: Ref = Ref::new(3);
const LOWER: Ref = Ref::new(4);

fn cframe(position: Vec3, rotation: [f32; 9]) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation,
    })
}

fn vector3(v: Vec3) -> Variant {
    Variant::Vector3(Vector3Data {
        x: v.x,
        y: v.y,
        z: v.z,
    })
}

fn bone(name: &str, parent: Option<u16>, position: [f32; 3]) -> Bone {
    Bone {
        name: name.to_owned(),
        parent,
        lod_parent: parent,
        culling: 0.0,
        rotation: IDENTITY,
        position,
    }
}

/// A strip two studs long down the native Y axis, its joint at the origin:
/// the top half weighted to `UpperArm`, the bottom half to `LowerArm`, the
/// joint row split between them, and one vertex on a bone (`Head`) no part
/// is named after. Normals point along +X.
fn strip() -> rbx_mesh::SkinnedMesh {
    let rows = [1.0f32, 0.5, 0.0, -0.5, -1.0];
    let mut vertices = Vec::new();
    let mut envelopes = Vec::new();
    for y in rows {
        for x in [-0.25f32, 0.25] {
            vertices.push(rbx_mesh::Vertex {
                position: [x, y, 0.0],
                normal: [1.0, 0.0, 0.0],
                uv: [0.0, 0.0],
                color: [255; 4],
            });
            let weights = match y {
                y if y > 0.0 => [255, 0, 0, 0],
                0.0 => [128, 127, 0, 0],
                _ => [0, 255, 0, 0],
            };
            envelopes.push(Envelope {
                slots: [0, 1, 0, 0],
                weights,
            });
        }
    }
    vertices.push(rbx_mesh::Vertex {
        position: [0.0, -1.0, 0.25],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
        color: [255; 4],
    });
    envelopes.push(Envelope {
        slots: [2, 0, 0, 0],
        weights: [255, 0, 0, 0],
    });
    let skin = Skin::new(
        vec![
            bone("UpperArm", None, [0.0, 1.0, 0.0]),
            bone("LowerArm", Some(0), [0.0, 0.0, 0.0]),
            bone("Head", None, [0.0, 3.0, 0.0]),
        ],
        vec![Subset {
            faces: 0..1,
            vertices: 0..11,
            bones: vec![0, 1, 2],
        }],
        envelopes,
    );
    let mesh = rbx_mesh::Mesh {
        version: (4, 0),
        bounds: rbx_mesh::Aabb {
            min: [-0.25, -1.0, 0.0],
            max: [0.25, 1.0, 0.25],
        },
        vertices,
        indices: vec![0, 1, 9],
        lods: Vec::new(),
    };
    rbx_mesh::SkinnedMesh {
        mesh,
        skin: Some(skin),
    }
}

/// `Workspace > Model > {MeshPart UpperArm, Part LowerArm}`: the upper arm
/// draws the strip with its joint at (0, 5, 0), the lower arm hangs from
/// that joint. `scale` multiplies both parts' `Size` (the mesh's
/// `InitialSize` stays the native extent).
fn arm(scale: f32) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(1);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    dom.insert(Instance::new(MODEL, "Model", "Arm"));
    dom.set_parent(MODEL, Some(workspace));

    let mut upper = Instance::new(UPPER, "MeshPart", "UpperArm");
    let properties = upper.properties_mut();
    properties.insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://7".to_string()),
    );
    properties.insert(
        "size".to_string(),
        vector3(Vec3::new(0.5, 2.0, 0.25) * scale),
    );
    properties.insert(
        "InitialSize".to_string(),
        vector3(Vec3::new(0.5, 2.0, 0.25)),
    );
    properties.insert(
        "CFrame".to_string(),
        cframe(Vec3::new(0.0, 5.0, 0.0), IDENTITY),
    );
    dom.insert(upper);
    dom.set_parent(UPPER, Some(MODEL));

    let mut lower = Instance::new(LOWER, "Part", "LowerArm");
    let properties = lower.properties_mut();
    properties.insert(
        "size".to_string(),
        vector3(Vec3::new(0.5, 1.0, 0.5) * scale),
    );
    properties.insert(
        "CFrame".to_string(),
        cframe(Vec3::new(0.0, 5.0 - 0.5 * scale, 0.0), IDENTITY),
    );
    dom.insert(lower);
    dom.set_parent(LOWER, Some(MODEL));
    dom
}

fn planned(dom: &WeakDom) -> Plan {
    let database = ReflectionDatabase::embedded();
    plan(dom, &database, &mut Catalog::new(dom, &database))
}

fn resolved(plan: &Plan) -> Resolved {
    let meshes = HashMap::from([(AssetRef::Id(7), Arc::new(strip()))]);
    resolve(plan, meshes, HashMap::new()).0
}

/// Turns the lower arm 90° about +Z around the joint at (0, 5, 0).
fn bend(dom: &mut WeakDom, scale: f32) {
    let joint = Vec3::new(0.0, 5.0, 0.0);
    // The centre hung 0.5·scale below the joint; a quarter turn about +Z
    // swings it out along +X.
    let centre = joint + Vec3::new(0.5 * scale, 0.0, 0.0);
    dom.get_mut(LOWER)
        .unwrap()
        .properties_mut()
        .insert("CFrame".to_string(), cframe(centre, QUARTER_Z));
}

fn positions(mesh: &rbx_mesh::Mesh) -> Vec<Vec3> {
    mesh.vertices
        .iter()
        .map(|v| Vec3::from(v.position))
        .collect()
}

fn assert_near(actual: Vec3, expected: Vec3) {
    assert!(
        actual.abs_diff_eq(expected, 1e-5),
        "{actual} is not {expected}"
    );
}

#[test]
fn a_skinned_mesh_part_draws_through_its_own_derived_mesh() {
    let plan = planned(&arm(1.0));
    let resolved = resolved(&plan);
    let instance = &resolved.instances[0];
    assert_eq!(instance.mesh, key(UPPER, &AssetRef::Id(7)));
    // At rest the derived mesh is the source itself, not a copy.
    assert!(Arc::ptr_eq(
        &resolved.meshes[&key(UPPER, &AssetRef::Id(7))],
        &resolved.meshes[&AssetRef::Id(7)]
    ));
    assert!(resolved.rigs.contains_key(&UPPER));
}

#[test]
fn every_matrix_is_identity_at_rest() {
    let plan = planned(&arm(1.0));
    let mut resolved = resolved(&plan);
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    for matrix in rig.palette(&plan.poses) {
        assert!(matrix.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{matrix}");
    }
    assert!(
        rig.pose(&plan.poses).is_none(),
        "nothing to re-upload at rest"
    );
}

#[test]
fn bending_the_child_moves_the_far_vertices_and_keeps_the_near_ones() {
    for scale in [1.0, 2.0] {
        let mut dom = arm(scale);
        let mut plan = planned(&dom);
        let mut resolved = resolved(&plan);
        bend(&mut dom, scale);
        plan.poses.track(&dom, LOWER);
        assert!(plan.poses.take_dirty());

        let rig = resolved.rigs.get_mut(&UPPER).unwrap();
        let posed = rig.pose(&plan.poses).expect("the lower arm moved");
        let rest = positions(&strip());
        let now = positions(&posed);

        // The upper rows follow `UpperArm`, which did not move.
        for i in 0..4 {
            assert_near(now[i], rest[i]);
        }
        // The lower rows swing a quarter turn about the joint: (x, y) → (−y, x),
        // in native units whatever the fit scale.
        for i in 6..10 {
            assert_near(now[i], Vec3::new(-rest[i].y, rest[i].x, 0.0));
            assert_near(Vec3::from(posed.vertices[i].normal), Vec3::Y);
        }
        // The joint row blends 128:127 between staying and turning.
        let (stay, turn) = (128.0 / 255.0, 127.0 / 255.0);
        let x = 0.25;
        assert_near(now[5], Vec3::new(x * stay, x * turn, 0.0));
        // A bone no sibling part is named after stays at its bind pose.
        assert_near(now[10], rest[10]);
        // The fit is computed from the source's bounds, never the pose's.
        assert_eq!(posed.bounds, strip().bounds);
        // The same pose again is nothing to do.
        assert!(rig.pose(&plan.poses).is_none());
    }
}

#[test]
fn moving_the_whole_model_rigidly_leaves_the_skin_alone() {
    let mut dom = arm(1.0);
    let mut plan = planned(&dom);
    let mut resolved = resolved(&plan);
    for (referent, y) in [(UPPER, 5.0), (LOWER, 4.5)] {
        dom.get_mut(referent).unwrap().properties_mut().insert(
            "CFrame".to_string(),
            cframe(Vec3::new(10.0, y + 3.0, -2.0), IDENTITY),
        );
        plan.poses.track(&dom, referent);
    }
    assert!(plan.poses.take_dirty());
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    assert!(rig.pose(&plan.poses).is_none());
}

#[test]
fn a_mesh_part_outside_a_model_or_without_bones_is_not_rigged() {
    // Outside a model: the same mesh straight under `Workspace`.
    let mut dom = arm(1.0);
    let workspace = dom.parent(MODEL).unwrap();
    dom.set_parent(UPPER, Some(workspace));
    let plan = planned(&dom);
    let resolved = resolved(&plan);
    assert!(resolved.rigs.is_empty());
    assert_eq!(resolved.instances[0].mesh, AssetRef::Id(7));

    // In the model but unskinned.
    let plan = planned(&arm(1.0));
    let mut plain = strip();
    plain.skin = None;
    let meshes = HashMap::from([(AssetRef::Id(7), Arc::new(plain))]);
    let (resolved, _) = resolve(&plan, meshes, HashMap::new());
    assert!(resolved.rigs.is_empty());
    assert_eq!(resolved.instances[0].mesh, AssetRef::Id(7));
}

#[test]
fn deform_blends_weights_and_renormalizes_them() {
    let mut mesh = strip();
    // Vertex 0 split 1:1 between a bone that stays and one shifted +2 in x,
    // with weights that sum to less than one.
    if let Some(skin) = &mut mesh.skin {
        skin.influences[0] = rbx_mesh::Influences {
            bones: [0, 1, 0, 0],
            weights: [0.25, 0.25, 0.0, 0.0],
        };
    }
    let palette = [
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        Mat4::IDENTITY,
    ];
    let out = deform(&mesh, &palette);
    assert_near(
        Vec3::from(out.vertices[0].position),
        Vec3::new(0.75, 1.0, 0.0),
    );
    // A translation leaves the normal alone.
    assert_near(Vec3::from(out.vertices[0].normal), Vec3::X);
}

/// The entry a patch re-plans for `referent`, installed in `plan`.
fn replanned(dom: &WeakDom, plan: &mut Plan, referent: Ref) -> Entry {
    let database = ReflectionDatabase::embedded();
    let entry =
        crate::scene::filemesh::replan(dom, &database, referent, &mut Catalog::new(dom, &database))
            .expect("a mesh part");
    plan.install(referent, Some(entry.clone()));
    entry
}

/// What a patch does for one mesh part: re-plan it, then bind its rig.
fn patch_in(dom: &WeakDom, plan: &mut Plan, resolved: &mut Resolved, referent: Ref) -> Binding {
    let database = ReflectionDatabase::embedded();
    let entry = replanned(dom, plan, referent);
    let stale = plan.rebinds.remove(&referent);
    bind(dom, &database, &mut plan.poses, resolved, &entry, stale)
}

/// Every mesh the tests here resolve against: the strip as asset 7 and as
/// asset 8.
fn landed() -> HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>> {
    HashMap::from([
        (AssetRef::Id(7), Arc::new(strip())),
        (AssetRef::Id(8), Arc::new(strip())),
    ])
}

#[test]
fn a_rig_patched_into_an_open_place_binds_and_bends() {
    // Opened with no arm at all; the arm then arrives as a patch, with its
    // mesh already resident.
    let mut dom = arm(1.0);
    dom.remove(MODEL);
    let mut plan = planned(&dom);
    let (mut resolved, _) = resolve(&plan, landed(), HashMap::new());
    assert!(!plan.poses.watches(MODEL));

    let mut dom = arm(1.0);
    let database = ReflectionDatabase::embedded();
    // The lower arm is patched in before the mesh part: its model is not
    // watched yet, so it is nothing to the poses.
    assert!(plan.poses.note(&dom, &database, LOWER).is_empty());
    let bound = patch_in(&dom, &mut plan, &mut resolved, UPPER);
    assert_eq!(
        bound.models,
        vec![MODEL],
        "its model is watched from now on"
    );
    assert_eq!(bound.key, Some(key(UPPER, &AssetRef::Id(7))));
    assert!(resolved.rigs.contains_key(&UPPER));

    bend(&mut dom, 1.0);
    plan.poses.track(&dom, LOWER);
    assert!(plan.poses.take_dirty());
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    assert!(rig.pose(&plan.poses).is_some(), "the lower arm bends it");
}

#[test]
fn a_mesh_part_streamed_in_later_binds_when_its_mesh_lands() {
    let dom = arm(1.0);
    let mut plan = planned(&arm(1.0));
    plan.install(UPPER, None);
    plan.poses = Poses::default();
    let mut resolved = resolve(&plan, HashMap::new(), HashMap::new()).0;
    // Patched in before its mesh has landed: no rig, but its model watched.
    let bound = patch_in(&dom, &mut plan, &mut resolved, UPPER);
    assert!(bound.key.is_none());
    assert!(plan.poses.watches(MODEL));
    // The landing resolves the plan again, and the rig is there.
    let (resolved, _) = resolve(&plan, landed(), HashMap::new());
    assert!(resolved.rigs.contains_key(&UPPER));
    assert_eq!(resolved.instances[0].mesh, key(UPPER, &AssetRef::Id(7)));
}

#[test]
fn a_new_mesh_id_rebinds_under_a_key_of_its_own() {
    let mut dom = arm(1.0);
    let mut plan = planned(&dom);
    let (mut resolved, _) = resolve(&plan, landed(), HashMap::new());
    let old = key(UPPER, &AssetRef::Id(7));
    assert!(resolved.meshes.contains_key(&old));

    dom.get_mut(UPPER).unwrap().properties_mut().insert(
        "MeshId".to_string(),
        Variant::String("rbxassetid://8".to_string()),
    );
    let bound = patch_in(&dom, &mut plan, &mut resolved, UPPER);
    let new = key(UPPER, &AssetRef::Id(8));
    assert_eq!(bound.key, Some(new.clone()));
    assert_eq!(resolved.rigs[&UPPER].asset, AssetRef::Id(8));
    assert!(resolved.meshes.contains_key(&new));
    assert!(
        !resolved.meshes.contains_key(&old),
        "the old pose is dropped"
    );

    // The same entry again is nothing to rebuild.
    let again = patch_in(&dom, &mut plan, &mut resolved, UPPER);
    assert!(again.key.is_none());
}

#[test]
fn a_sibling_renamed_added_or_removed_rebinds_the_model() {
    let database = ReflectionDatabase::embedded();
    let mut dom = arm(1.0);
    let mut plan = planned(&dom);
    let (mut resolved, _) = resolve(&plan, landed(), HashMap::new());

    // A move alone changes no binding.
    bend(&mut dom, 1.0);
    assert!(plan.poses.note(&dom, &database, LOWER).is_empty());

    // Renamed: no bone finds `LowerArm` any more.
    dom.set_name(LOWER, "Forearm").unwrap();
    let models = plan.poses.note(&dom, &database, LOWER);
    assert_eq!(models, vec![MODEL]);
    plan.rebind_models(&models);
    assert!(
        plan.rebinds.contains(&UPPER),
        "the mesh part is owed a re-bind"
    );
    let bound = patch_in(&dom, &mut plan, &mut resolved, UPPER);
    assert!(bound.key.is_some(), "rebuilt");
    assert!(plan.rebinds.is_empty());
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    assert!(
        rig.pose(&plan.poses).is_none(),
        "the bent part no longer binds a bone"
    );

    // A part named for it added: bound again, and bent at once.
    let added = Ref::new(9);
    let mut part = Instance::new(added, "Part", "LowerArm");
    part.properties_mut().insert(
        "CFrame".to_string(),
        cframe(Vec3::new(0.0, 4.5, 0.0), IDENTITY),
    );
    dom.insert(part);
    dom.set_parent(added, Some(MODEL));
    assert_eq!(plan.poses.note(&dom, &database, added), vec![MODEL]);
    plan.rebind_models(&[MODEL]);
    patch_in(&dom, &mut plan, &mut resolved, UPPER);
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    assert!(rig.bones[1].as_ref().is_some_and(|bone| bone.part == added));

    // Removed: the bone lets go of it.
    dom.remove(added);
    assert_eq!(plan.poses.forget_part(added), Some(MODEL));
    assert!(plan.poses.forget_part(added).is_none(), "once");
    assert!(!plan.poses.rest.contains_key(&added));
}

#[test]
fn a_model_deleted_whole_is_let_go_of() {
    let mut dom = arm(1.0);
    let mut plan = planned(&dom);
    let (mut resolved, _) = resolve(&plan, landed(), HashMap::new());
    dom.remove(MODEL);
    for part in [UPPER, LOWER] {
        resolved.drop_rig(part);
        plan.poses.forget_part(part);
    }
    assert!(!plan.poses.watches(MODEL));
    assert!(plan.poses.members.is_empty());
    assert!(resolved.rigs.is_empty());
    assert!(!resolved.meshes.contains_key(&key(UPPER, &AssetRef::Id(7))));
}

#[test]
fn a_place_saved_mid_pose_rests_where_its_joints_say() {
    // Saved bent, with a `Motor6D` from the upper to the lower arm whose
    // C0/C1 hang the lower arm 0.5 below the joint at the upper arm's
    // origin: the rest pose is the joints', and the skin opens bent.
    let mut dom = arm(1.0);
    bend(&mut dom, 1.0);
    let motor = Ref::new(10);
    let mut joint = Instance::new(motor, "Motor6D", "Elbow");
    let properties = joint.properties_mut();
    properties.insert("Part0".to_string(), Variant::Ref(UPPER));
    properties.insert("Part1".to_string(), Variant::Ref(LOWER));
    properties.insert("C0".to_string(), cframe(Vec3::ZERO, IDENTITY));
    properties.insert("C1".to_string(), cframe(Vec3::new(0.0, 0.5, 0.0), IDENTITY));
    dom.insert(joint);
    dom.set_parent(motor, Some(UPPER));

    let plan = planned(&dom);
    assert_near(
        plan.poses.rest[&LOWER].w_axis.truncate(),
        Vec3::new(0.0, 4.5, 0.0),
    );
    let mut resolved = resolved(&plan);
    let rig = resolved.rigs.get_mut(&UPPER).unwrap();
    let posed = rig.pose(&plan.poses).expect("opened bent");
    let rest = positions(&strip());
    let now = positions(&posed);
    for (now, rest) in now.iter().zip(&rest).skip(6).take(4) {
        assert_near(*now, Vec3::new(-rest.y, rest.x, 0.0));
    }
}

/// Points as an unskinned mesh, every normal facing +Z.
fn points(points: &[[f32; 3]]) -> Arc<rbx_mesh::SkinnedMesh> {
    Arc::new(rbx_mesh::SkinnedMesh::from(rbx_mesh::Mesh {
        version: (4, 1),
        vertices: points
            .iter()
            .map(|&position| rbx_mesh::Vertex {
                position,
                normal: [0.0, 0.0, 1.0],
                uv: [0.0; 2],
                color: [255; 4],
            })
            .collect(),
        indices: Vec::new(),
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-1.0; 3],
            max: [1.0; 3],
        },
    }))
}

const HANDLE: Ref = Ref::new(6);

/// `Model > {MeshPart UpperArm (y 5..7), MeshPart LowerArm (y 3..5),
/// Accessory > MeshPart Handle}`: each arm part carries a `WrapTarget` cage
/// along its axis, the handle a `WrapLayer` whose reference is that body,
/// and a garment strip from y 3 to 7, welded to the upper arm.
fn dressed_arm() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(1);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    dom.insert(Instance::new(MODEL, "Model", "Character"));
    dom.set_parent(MODEL, Some(workspace));
    let mut add =
        |referent: Ref, class: &str, name: &str, parent: Ref, props: Vec<(&str, Variant)>| {
            let mut instance = Instance::new(referent, class, name);
            for (key, value) in props {
                instance.properties_mut().insert(key.to_string(), value);
            }
            dom.insert(instance);
            dom.set_parent(referent, Some(parent));
        };
    let asset = |id: u32| Variant::String(format!("rbxassetid://{id}"));
    let part = |mesh: u32, height: f32, y: f32| {
        vec![
            ("MeshId", asset(mesh)),
            ("size", vector3(Vec3::new(1.0, height, 1.0))),
            ("InitialSize", vector3(Vec3::new(1.0, height, 1.0))),
            ("CFrame", cframe(Vec3::new(0.0, y, 0.0), IDENTITY)),
        ]
    };
    add(UPPER, "MeshPart", "UpperArm", MODEL, part(30, 2.0, 6.0));
    add(LOWER, "MeshPart", "LowerArm", MODEL, part(30, 2.0, 4.0));
    add(
        Ref::new(10),
        "WrapTarget",
        "WrapTarget",
        UPPER,
        vec![("CageMeshId", asset(31))],
    );
    add(
        Ref::new(11),
        "WrapTarget",
        "WrapTarget",
        LOWER,
        vec![("CageMeshId", asset(31))],
    );
    add(Ref::new(5), "Accessory", "Shirt", MODEL, Vec::new());
    add(
        HANDLE,
        "MeshPart",
        "Handle",
        Ref::new(5),
        part(32, 4.0, 5.0),
    );
    add(
        Ref::new(7),
        "WrapLayer",
        "WrapLayer",
        HANDLE,
        vec![("ReferenceMeshId", asset(33))],
    );
    add(
        Ref::new(8),
        "Weld",
        "AccessoryWeld",
        HANDLE,
        vec![
            ("Part0", Variant::Ref(UPPER)),
            ("Part1", Variant::Ref(HANDLE)),
        ],
    );
    dom
}

fn column(x: f32, from: f32, to: f32, step: f32) -> Vec<[f32; 3]> {
    let count = ((to - from) / step).round() as usize;
    (0..=count)
        .map(|i| [x, from + i as f32 * step, 0.0])
        .collect()
}

#[test]
fn a_layered_garment_follows_the_part_it_wraps() {
    let mut dom = dressed_arm();
    let mut plan = planned(&dom);
    let meshes = HashMap::from([
        (AssetRef::Id(30), points(&[[0.0; 3]])),
        (AssetRef::Id(31), points(&column(0.0, -1.0, 1.0, 0.25))),
        (AssetRef::Id(32), points(&column(0.5, -2.0, 2.0, 0.5))),
        (AssetRef::Id(33), points(&column(0.0, -2.0, 2.0, 0.25))),
    ]);
    let mut resolved = resolve(&plan, meshes, HashMap::new()).0;
    let rig = resolved
        .rigs
        .get_mut(&HANDLE)
        .expect("the garment is rigged");
    assert!(rig.wraps());
    let skin = rig.source().skin.as_ref().expect("the body lent its skin");
    let names: Vec<&str> = skin.bones.iter().map(|bone| bone.name.as_str()).collect();
    assert_eq!(names, ["UpperArm", "LowerArm"]);
    let rest = positions(rig.source());

    // Turn the lower arm a quarter about +Z around the elbow at (0, 5, 0).
    dom.get_mut(LOWER).unwrap().properties_mut().insert(
        "CFrame".to_string(),
        cframe(Vec3::new(1.0, 5.0, 0.0), QUARTER_Z),
    );
    plan.poses.track(&dom, LOWER);
    let posed = positions(&rig.pose(&plan.poses).expect("the lower arm moved"));
    // Handle-local (0.5, -2) hangs at the wrist: (x, y) → (−y, x) about the
    // elbow, the handle at the elbow. The shoulder end stays.
    let (wrist, shoulder) = (0, rest.len() - 1);
    let turned = Vec3::new(-rest[wrist].y, rest[wrist].x, rest[wrist].z);
    assert!(
        posed[wrist].abs_diff_eq(turned, 1e-3),
        "{} is not {turned}",
        posed[wrist]
    );
    assert!(posed[shoulder].abs_diff_eq(rest[shoulder], 1e-5));

    // A patch that changes nothing keeps the rig; the instance draws it.
    let entry = replanned(&dom, &mut plan, HANDLE);
    let bound = bind(
        &dom,
        &ReflectionDatabase::embedded(),
        &mut plan.poses,
        &mut resolved,
        &entry,
        false,
    );
    assert!(bound.key.is_none(), "kept, not rebuilt");
    assert!(resolved.rigs.get(&HANDLE).is_some_and(Rig::wraps));
}
