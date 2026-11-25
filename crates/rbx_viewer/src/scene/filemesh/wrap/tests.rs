use super::*;

fn mesh(points: &[[f32; 3]]) -> Arc<rbx_mesh::SkinnedMesh> {
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

fn id(n: u64) -> AssetRef {
    AssetRef::Id(n)
}

fn wrap(order: f32) -> Wrap {
    Wrap {
        reference: id(1),
        origin: Mat4::IDENTITY,
        outer: None,
        order,
        bind: None,
        targets: vec![Target {
            cage: id(2),
            origin: Mat4::IDENTITY,
            mesh: id(3),
            fit: Fit::Part {
                cframe: Mat4::IDENTITY,
                size: Vec3::ONE,
                initial_size: Some(Vec3::ONE),
            },
            part: "UpperTorso".into(),
            referent: Ref::new(5),
            hsr: None,
        }],
        hsr: None,
        character: Ref::new(0),
    }
}

fn meshes(body: &[[f32; 3]]) -> HashMap<AssetRef, Arc<rbx_mesh::SkinnedMesh>> {
    let reference = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]];
    HashMap::from([
        (id(1), mesh(&reference)),
        (id(2), mesh(body)),
        (id(3), mesh(&[])),
    ])
}

fn moved(order: f32, body: &[[f32; 3]]) -> Vec3 {
    let garment = mesh(&[[0.5, 1.0, 0.0]]);
    let out = deform(&wrap(order), Mat4::IDENTITY, &garment, &meshes(body)).unwrap();
    Vec3::from(out.vertices[0].position)
}

#[test]
fn a_body_the_reference_fits_leaves_the_garment_where_it_is() {
    let at = moved(-1.0, &[[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]]);
    assert!(at.distance(Vec3::new(0.5, 1.0, 0.0)) < 1e-5, "{at}");
}

#[test]
fn a_wider_body_carries_the_garment_out_with_it() {
    let at = moved(-1.0, &[[0.4, 0.0, 0.0], [0.4, 1.0, 0.0], [0.4, 2.0, 0.0]]);
    assert!(at.distance(Vec3::new(0.9, 1.0, 0.0)) < 1e-5, "{at}");
}

#[test]
fn a_higher_order_sits_further_out() {
    let body = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]];
    assert!(moved(2.0, &body).z > moved(0.0, &body).z);
}

#[test]
fn without_its_cages_the_garment_is_left_alone() {
    let garment = mesh(&[[0.0; 3]]);
    assert!(deform(&wrap(0.0), Mat4::IDENTITY, &garment, &HashMap::new()).is_none());
}

/// The meshes a jacket worn on a one-part rig asks for, with its cages saved
/// under `cage` (the body's) and `reference` (the jacket's).
fn worn_garment_wants(cage: &str, reference: &str) -> Vec<AssetRef> {
    use rbx_dom::{CFrameData, Instance, Vector3Data};
    let mut dom = WeakDom::new();
    let add =
        |dom: &mut WeakDom, n: u32, class: &str, parent: Option<Ref>, props: &[(&str, Variant)]| {
            let referent = Ref::new(n);
            let mut instance = Instance::new(referent, class, class);
            for (key, value) in props {
                instance
                    .properties_mut()
                    .insert((*key).into(), value.clone());
            }
            dom.insert(instance);
            dom.set_parent(referent, parent);
            referent
        };
    let at = Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    });
    let size = Variant::Vector3(Vector3Data {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    });
    let content = |n: u64| Variant::String(format!("rbxassetid://{n}"));
    let mesh_part = |id: u64| {
        [
            ("MeshId", content(id)),
            ("size", size.clone()),
            ("CFrame", at.clone()),
        ]
    };
    let workspace = add(&mut dom, 1, "Workspace", None, &[]);
    let rig = add(&mut dom, 2, "Model", Some(workspace), &[]);
    let torso = add(&mut dom, 3, "MeshPart", Some(rig), &mesh_part(10));
    add(
        &mut dom,
        4,
        "WrapTarget",
        Some(torso),
        &[(cage, content(11))],
    );
    let jacket = add(&mut dom, 5, "Accessory", Some(rig), &[]);
    let handle = add(&mut dom, 6, "MeshPart", Some(jacket), &mesh_part(20));
    add(
        &mut dom,
        7,
        "WrapLayer",
        Some(handle),
        &[(reference, content(21))],
    );

    let database = ReflectionDatabase::embedded();
    let wrap = of(&dom, &database, handle).unwrap();
    wrap.meshes().cloned().collect()
}

#[test]
fn a_worn_layered_garment_asks_for_its_cages() {
    assert_eq!(
        worn_garment_wants("CageMeshId", "ReferenceMeshId"),
        vec![id(21), id(11), id(10)]
    );
}

#[test]
fn cages_saved_as_content_properties_are_read_too() {
    assert_eq!(
        worn_garment_wants("CageMeshContent", "ReferenceMeshContent"),
        vec![id(21), id(11), id(10)]
    );
}

#[test]
fn a_reference_standing_off_the_body_is_slid_onto_it_first() {
    // Uneven rungs, so no other vertex is a nearer match across the gap.
    let rungs: Vec<[f32; 3]> = (0..12)
        .flat_map(|i| {
            let y = (i * i) as f32 * 0.05;
            [[-0.3, y, 0.0], [0.3, y, 0.1]]
        })
        .collect();
    let lifted: Vec<[f32; 3]> = rungs.iter().map(|p| [p[0], p[1] + 0.8, p[2]]).collect();
    let meshes = HashMap::from([
        (id(1), mesh(&rungs)),
        (id(2), mesh(&lifted)),
        (id(3), mesh(&[])),
    ]);
    let garment = mesh(&[[0.0, 2.0, 0.05], [0.0, 3.0, 0.05]]);
    let out = deform(&wrap(-1.0), Mat4::IDENTITY, &garment, &meshes).unwrap();
    for (before, after) in garment.vertices.iter().zip(&out.vertices) {
        let moved = Vec3::from(after.position) - Vec3::from(before.position);
        assert!(moved.distance(Vec3::new(0.0, 0.8, 0.0)) < 0.02, "{moved}");
    }
}

#[test]
fn bind_offset_turns_the_garment_about_the_root_part() {
    use rbx_dom::{CFrameData, Instance, Vector3Data};
    let cframe = |y: f32, rotation: [f32; 9]| {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x: 0.0, y, z: 0.0 },
            rotation,
        })
    };
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    // A quarter turn about Z: X goes to Y.
    let quarter = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let mut dom = WeakDom::new();
    let mut add =
        |n: u32, class: &str, name: &str, parent: Option<Ref>, props: Vec<(&str, Variant)>| {
            let mut instance = Instance::new(Ref::new(n), class, name);
            for (key, value) in props {
                instance.properties_mut().insert(key.into(), value);
            }
            dom.insert(instance);
            dom.set_parent(Ref::new(n), parent);
            Ref::new(n)
        };
    let content = |n: u64| Variant::String(format!("rbxassetid://{n}"));
    let size = Variant::Vector3(Vector3Data {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    });
    let rig = add(1, "Model", "Rig", None, vec![]);
    add(
        2,
        "Part",
        "HumanoidRootPart",
        Some(rig),
        vec![("CFrame", cframe(5.0, identity))],
    );
    let torso = add(
        3,
        "MeshPart",
        "UpperTorso",
        Some(rig),
        vec![
            ("MeshId", content(10)),
            ("size", size.clone()),
            ("CFrame", cframe(6.0, identity)),
        ],
    );
    add(
        4,
        "WrapTarget",
        "WrapTarget",
        Some(torso),
        vec![("CageMeshId", content(11))],
    );
    let jacket = add(5, "Accessory", "Jacket", Some(rig), vec![]);
    let handle = add(
        6,
        "MeshPart",
        "Handle",
        Some(jacket),
        vec![
            ("MeshId", content(20)),
            ("size", size),
            ("CFrame", cframe(6.0, identity)),
        ],
    );
    add(
        7,
        "WrapLayer",
        "WrapLayer",
        Some(handle),
        vec![
            ("ReferenceMeshId", content(21)),
            ("BindOffset", cframe(0.0, quarter)),
        ],
    );

    let wrap = of(&dom, &ReflectionDatabase::embedded(), handle).unwrap();
    let bind = wrap.bind.expect("a BindOffset that is not the identity");
    // The root part stays put; a point a stud to its side swings above it.
    assert!(
        bind.transform_point3(Vec3::new(0.0, 5.0, 0.0))
            .distance(Vec3::new(0.0, 5.0, 0.0))
            < 1e-5
    );
    let swung = bind.transform_point3(Vec3::new(1.0, 5.0, 0.0));
    assert!(swung.distance(Vec3::new(0.0, 6.0, 0.0)) < 1e-5, "{swung}");
    // The garment moves with it, in its own mesh space.
    let moved = rebound(&mesh(&[[1.0, 5.0, 0.0]]).mesh, bind);
    assert!(Vec3::from(moved.vertices[0].position).distance(Vec3::new(0.0, 6.0, 0.0)) < 1e-5);
}
