use glam::Vec3;
use rbx_cloud::CloudError;
use rbx_dom::{Variant, WeakDom};

use super::resolve;

/// A `version 2.00` mesh of one triangle spanning `extent`.
fn mesh_bytes(extent: Vec3) -> Vec<u8> {
    let mut body = Vec::from(b"version 2.00\n".as_slice());
    body.extend_from_slice(&12u16.to_le_bytes());
    body.extend_from_slice(&[40, 12]);
    body.extend_from_slice(&3u32.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    for corner in [-extent / 2.0, extent / 2.0, Vec3::ZERO] {
        for c in corner.to_array() {
            body.extend_from_slice(&c.to_le_bytes());
        }
        body.extend(std::iter::repeat_n(0u8, 28));
    }
    for index in 0u32..3 {
        body.extend_from_slice(&index.to_le_bytes());
    }
    body
}

/// What Open Cloud's import hands back: a `Model` holding one `MeshPart`.
fn model_bytes(mesh: &str) -> Vec<u8> {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "frozen", None);
    let part = dom.new_instance("MeshPart", "Rock", Some(model));
    let _ = dom.set_property(part, "MeshId", Variant::String(mesh.into()));
    rbx_binary::serialize(&dom).unwrap()
}

#[test]
fn the_imported_mesh_is_found_and_measured() {
    let baked = Vec3::new(4.0, 2.0, 1.0);
    let fetch = |id: u64| {
        assert_eq!(id, 55);
        Ok(mesh_bytes(baked * 0.28))
    };
    let (id, native) = resolve(&model_bytes("rbxassetid://55"), fetch, baked).unwrap();
    assert_eq!(id, 55);
    assert!((native - baked * 0.28).length() < 1e-4);
}

#[test]
fn a_reshaped_import_is_refused() {
    let baked = Vec3::new(4.0, 2.0, 1.0);
    let fetch = |_| Ok(mesh_bytes(Vec3::new(4.0, 1.0, 2.0)));
    let err = resolve(&model_bytes("rbxassetid://55"), fetch, baked).unwrap_err();
    assert!(err.contains("proportions"), "{err}");
}

#[test]
fn an_import_without_a_mesh_says_so() {
    let fetch = |_| -> Result<Vec<u8>, String> { panic!("nothing to fetch") };
    let err = resolve(&model_bytes(""), fetch, Vec3::ONE).unwrap_err();
    assert!(err.contains("no MeshPart"), "{err}");
}

/// The whole round trip against Roblox: bakes a turned 4 × 2 × 1 box,
/// uploads it through the stored key (it needs `asset:read` and
/// `asset:write`), and checks the mesh Roblox made of it. Creates a real
/// `Model` asset in the key owner's inventory, hence ignored:
/// `cargo test -p rbx_studio --bin rbxstudio live_round_trip -- --ignored`.
#[test]
#[ignore]
fn live_round_trip() {
    use rbx_dom::{CFrameData, Vector3Data};
    use rbx_mesh::{Aabb, Mesh, Vertex};
    use rbx_reflection::ReflectionDatabase;

    let corners: Vec<Vertex> = (0..8)
        .map(|i| Vertex {
            position: [
                if i & 1 == 0 { -2.0 } else { 2.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -0.5 } else { 0.5 },
            ],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            color: [255; 4],
        })
        .collect();
    let mesh = Mesh {
        version: (2, 0),
        vertices: corners,
        indices: vec![
            0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4,
            6, 1, 3, 5, 3, 7, 5,
        ],
        lods: Vec::new(),
        bounds: Aabb {
            min: [-2.0, -1.0, -0.5],
            max: [2.0, 1.0, 0.5],
        },
    };
    let mut dom = WeakDom::new();
    let part = dom.new_instance("MeshPart", "FreezeRotationLiveTest", None);
    let (s, c) = (0.5f32, 0.75f32.sqrt());
    let turned = CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 5.0,
            z: 0.0,
        },
        rotation: [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c],
    };
    let _ = dom.set_property(part, "CFrame", Variant::CFrame(turned));
    let _ = dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 2.0,
            z: 1.0,
        }),
    );
    let _ = dom.set_property(part, "MeshId", Variant::String("rbxassetid://1".into()));
    let plan = crate::freeze::plan(&dom, &ReflectionDatabase::embedded(), part, &mesh).unwrap();
    let gltf = rbx_viewer::export::gltf(&plan.export).into_bytes();
    let (id, native) = super::upload(&plan, gltf).unwrap();
    eprintln!("mesh {id}, extent {native} for a bake of {}", plan.size);
}

#[test]
fn an_invalid_key_is_not_blamed_on_its_permissions() {
    let invalid = CloudError::Refused {
        status: 401,
        message: "Invalid API Key".into(),
    };
    assert!(super::describe(&invalid).contains("expired"));
    let scoped = CloudError::Refused {
        status: 403,
        message: "Insufficient scope".into(),
    };
    assert!(super::describe(&scoped).contains("asset:write"));
}
