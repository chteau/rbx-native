use serde_json::Value;

use crate::rig::Part;
use crate::split::Body;
use crate::test_humanoid::{humanoid, Pose};
use crate::write::{gltf, gltf_skinned};
use crate::Mesh;

fn json_chunk(glb: &[u8]) -> Value {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    assert_eq!(&glb[16..20], b"JSON");
    serde_json::from_slice(&glb[20..20 + len]).unwrap()
}

fn setup() -> (Mesh, Body, Vec<Mesh>) {
    let model = humanoid(Pose::A);
    let fit = crate::fit(&model.mesh, 5.2).unwrap();
    let body = crate::split(&model.mesh, &fit).unwrap();
    let cages = body
        .parts
        .iter()
        .map(|p| {
            let mut m = p.mesh.clone();
            for v in &mut m.positions {
                *v *= 1.1;
            }
            m
        })
        .collect();
    (model.mesh, body, cages)
}

fn node<'a>(doc: &'a Value, name: &str) -> Option<&'a Value> {
    doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == name)
}

fn count_cages(doc: &Value) -> usize {
    doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| {
            n["name"]
                .as_str()
                .is_some_and(|s| s.ends_with("_OuterCage"))
        })
        .count()
}

#[test]
fn rigid_cages_sit_with_their_parts() {
    let (_, body, cages) = setup();
    let with = json_chunk(&gltf(&body, &cages));
    let without = json_chunk(&gltf(&body, &[]));
    assert_eq!(count_cages(&without), 0);
    assert_eq!(without["nodes"].as_array().unwrap().len(), 15);
    assert_eq!(count_cages(&with), 15);
    for part in Part::ALL {
        let cage = node(&with, &format!("{}_OuterCage", part.name())).unwrap();
        let geo = node(&with, &format!("{}_Geo", part.name())).unwrap();
        assert_eq!(cage["translation"], geo["translation"]);
    }
}

#[test]
fn rigid_cage_triangles_are_read_back() {
    let (_, body, cages) = setup();
    let read = crate::read_bytes(&gltf(&body, &cages), crate::Format::Gltf).unwrap();
    let expected: usize = body
        .parts
        .iter()
        .map(|p| p.mesh.valid_triangles().count())
        .chain(cages.iter().map(|c| c.valid_triangles().count()))
        .sum();
    assert_eq!(read.triangles.len(), expected);
}

#[test]
fn skinned_cages_are_skinned_like_parts() {
    let (mesh, body, cages) = setup();
    let fit = crate::fit(&mesh, 5.2).unwrap();
    let skin = crate::weights(&mesh, &fit, &body);
    let with = json_chunk(&gltf_skinned(&body, &skin, &cages));
    let without = json_chunk(&gltf_skinned(&body, &skin, &[]));
    assert_eq!(count_cages(&without), 0);
    assert_eq!(
        with["nodes"].as_array().unwrap().len(),
        without["nodes"].as_array().unwrap().len() + 15
    );
    assert_eq!(count_cages(&with), 15);
    for part in Part::ALL {
        let cage = node(&with, &format!("{}_OuterCage", part.name())).unwrap();
        assert_eq!(cage["skin"], 0);
        let mesh = &with["meshes"][cage["mesh"].as_u64().unwrap() as usize];
        let attributes = &mesh["primitives"][0]["attributes"];
        assert!(attributes["JOINTS_0"].is_u64());
        assert!(attributes["WEIGHTS_0"].is_u64());
    }
}
