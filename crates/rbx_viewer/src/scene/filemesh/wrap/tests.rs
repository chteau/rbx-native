use super::*;

fn mesh(points: &[[f32; 3]]) -> Arc<rbx_mesh::Mesh> {
    Arc::new(rbx_mesh::Mesh {
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
    })
}

fn id(n: u64) -> AssetRef {
    AssetRef::Id(n)
}

fn wrap(order: f32) -> Wrap {
    Wrap {
        reference: id(1),
        origin: Mat4::IDENTITY,
        order,
        targets: vec![Target {
            cage: id(2),
            origin: Mat4::IDENTITY,
            mesh: id(3),
            fit: Fit::Part {
                cframe: Mat4::IDENTITY,
                size: Vec3::ONE,
                initial_size: Some(Vec3::ONE),
            },
        }],
    }
}

fn meshes(body: &[[f32; 3]]) -> HashMap<AssetRef, Arc<rbx_mesh::Mesh>> {
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

#[test]
fn a_worn_layered_garment_asks_for_its_cages() {
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
        &[("CageMeshId", content(11))],
    );
    let jacket = add(&mut dom, 5, "Accessory", Some(rig), &[]);
    let handle = add(&mut dom, 6, "MeshPart", Some(jacket), &mesh_part(20));
    add(
        &mut dom,
        7,
        "WrapLayer",
        Some(handle),
        &[("ReferenceMeshId", content(21))],
    );

    let database = ReflectionDatabase::embedded();
    let wrap = of(&dom, &database, handle).unwrap();
    let wanted: Vec<_> = wrap.meshes().cloned().collect();
    assert_eq!(wanted, vec![id(21), id(11), id(10)]);
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
