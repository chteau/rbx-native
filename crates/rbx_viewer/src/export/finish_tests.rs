//! The looks one UV set and one colour cannot carry: a material pack across
//! a tilted facet, under a mesh's image, the procedural materials, and a
//! union drawn as its pieces.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Variant, Vector3Data};
use serde_json::Value;

use super::*;
use crate::pick::{Pack, Piece};
use crate::scene::{Kind, ShapeKind};

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn part(dom: &mut WeakDom, class: &str, size: [f32; 3]) -> Ref {
    let referent = dom.new_instance(class, class, None);
    dom.set_property(
        referent,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: IDENTITY,
        }),
    )
    .unwrap();
    let [x, y, z] = size;
    dom.set_property(referent, "size", Variant::Vector3(Vector3Data { x, y, z }))
        .unwrap();
    referent
}

fn checker() -> Pack {
    let pixels = (0..16)
        .flat_map(|i| {
            if (i % 4 + i / 4) % 2 == 0 {
                [200, 60, 60, 255]
            } else {
                [60, 60, 200, 255]
            }
        })
        .collect();
    Pack {
        maps: [
            Some(Arc::new(Image {
                width: 4,
                height: 4,
                pixels,
            })),
            None,
            None,
            None,
        ],
        studs_per_tile: 2.0,
    }
}

fn export(dom: &WeakDom, meshes: &Meshes, root: Ref) -> Export {
    meshes_of(dom, &ReflectionDatabase::embedded(), meshes, &[root])
}

fn document(exported: &Export) -> Value {
    serde_json::from_str(&gltf(exported)).unwrap()
}

/// A wedge's four upright faces take one projection each and tile the
/// pack's own image; its slope, at 45 degrees between two axes, is blended
/// in the viewport and so bakes, with tangents for its normal map.
#[test]
fn a_wedges_slope_bakes_and_its_upright_faces_tile() {
    let mut dom = WeakDom::new();
    let wedge = part(&mut dom, "WedgePart", [4.0, 2.0, 2.0]);
    let meshes = Meshes::default().with_materials(HashMap::from([(wedge, Arc::new(checker()))]));

    let exported = export(&dom, &meshes, wedge);

    assert_eq!(
        exported.meshes.len(),
        2,
        "tiled faces, then the baked slope"
    );
    let (tiled, baked) = (&exported.meshes[0], &exported.meshes[1]);
    assert_eq!(tiled.tangents.len(), tiled.positions.len());
    assert_eq!(baked.tangents.len(), baked.positions.len());
    assert_eq!(baked.indices.len(), 6, "the slope's two triangles");
    assert_ne!(
        tiled.maps.color, baked.maps.color,
        "the slope reads its own bake"
    );
    assert!(baked.uvs.iter().flatten().all(|c| (0.0..=1.0).contains(c)));
    // Both under one node, placed by the part's own CFrame.
    assert_eq!(exported.nodes.len(), 1);
    assert_eq!(exported.nodes[0].meshes, [0, 1]);
    assert_eq!(exported.nodes[0].placement.unwrap().1, [1.0, 2.0, 3.0]);

    let document = document(&exported);
    let primitives = document["meshes"][0]["primitives"].as_array().unwrap();
    assert_eq!(primitives.len(), 2);
    assert!(primitives[1]["attributes"]["TANGENT"].is_u64());
    let tangent =
        &document["accessors"][primitives[1]["attributes"]["TANGENT"].as_u64().unwrap() as usize];
    assert_eq!(tangent["type"], "VEC4");
}

/// Two wedges of the same size and pack share one bake, wherever they are.
#[test]
fn alike_parts_share_one_bake() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Pair", None);
    let a = part(&mut dom, "WedgePart", [4.0, 2.0, 2.0]);
    let b = part(&mut dom, "WedgePart", [4.0, 2.0, 2.0]);
    dom.set_parent(a, Some(model));
    dom.set_parent(b, Some(model));
    let pack = Arc::new(checker());
    let meshes =
        Meshes::default().with_materials(HashMap::from([(a, Arc::clone(&pack)), (b, pack)]));

    let exported = export(&dom, &meshes, model);

    let baked: Vec<_> = exported
        .meshes
        .iter()
        .filter(|m| m.indices.len() == 6)
        .collect();
    assert_eq!(baked.len(), 2);
    assert_eq!(baked[0].maps, baked[1].maps);
    // The pack's colour map, and one baked colour map.
    assert_eq!(exported.textures.len(), 2);
}

/// A textured mesh whose `Material` has a pack too: the viewport multiplies
/// the two, so the export bakes them into one colour map rather than
/// dropping the pack.
#[test]
fn a_textured_mesh_with_a_pack_bakes_both() {
    let mut dom = WeakDom::new();
    let rock = part(&mut dom, "MeshPart", [2.0, 2.0, 2.0]);
    dom.set_property(rock, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    let vertex = |position: [f32; 3], uv: [f32; 2]| rbx_mesh::Vertex {
        position,
        normal: [0.0, 1.0, 0.0],
        uv,
        color: [255; 4],
    };
    let mesh = rbx_mesh::SkinnedMesh::from(rbx_mesh::Mesh {
        version: (4, 1),
        vertices: vec![
            vertex([-1.0, 0.0, -1.0], [0.0, 0.0]),
            vertex([1.0, 0.0, -1.0], [1.0, 0.0]),
            vertex([0.0, 0.0, 1.0], [0.5, 1.0]),
        ],
        indices: vec![0, 2, 1],
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-1.0, 0.0, -1.0],
            max: [1.0, 0.0, 1.0],
        },
    });
    let grey = Arc::new(Image {
        width: 2,
        height: 2,
        pixels: [128, 128, 128, 255].repeat(4),
    });
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(mesh))]),
        HashMap::from([(rock, Arc::clone(&grey))]),
    )
    .with_materials(HashMap::from([(rock, Arc::new(checker()))]));

    let exported = export(&dom, &meshes, rock);

    assert_eq!(exported.meshes.len(), 1);
    let mesh = &exported.meshes[0];
    assert!(!mesh.tangents.is_empty(), "baked, even facing straight up");
    let color = mesh.maps.color.unwrap();
    // Not the image as downloaded: the bake made a colour map of its own.
    let decoded = png::Decoder::new(std::io::Cursor::new(&exported.textures[color]))
        .read_info()
        .unwrap();
    assert!(decoded.info().width > 2);
}

#[test]
fn neon_glows_by_the_viewports_own_factor() {
    let mut dom = WeakDom::new();
    let lamp = part(&mut dom, "Part", [1.0, 1.0, 1.0]);
    dom.set_property(
        lamp,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let meshes = Meshes::default().with_kinds(HashMap::from([(lamp, Kind::Neon)]));

    let exported = export(&dom, &meshes, lamp);
    let document = document(&exported);

    assert_eq!(exported.meshes[0].finish, Finish::Neon);
    let material = &document["materials"][0];
    assert_eq!(
        material["emissiveFactor"],
        serde_json::json!([1.0, 0.0, 0.0])
    );
    assert_eq!(
        material["pbrMetallicRoughness"]["baseColorFactor"],
        serde_json::json!([0.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        material["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"],
        6.0
    );
    assert!(document["extensionsUsed"]
        .as_array()
        .unwrap()
        .contains(&"KHR_materials_emissive_strength".into()));
    let mtl = mtl(&exported.meshes, "x");
    assert!(mtl.contains("Ke 1 0 0"));
}

#[test]
fn glass_transmits_what_its_transparency_lets_through() {
    let mut dom = WeakDom::new();
    let pane = part(&mut dom, "Part", [4.0, 4.0, 0.2]);
    dom.set_property(pane, "Transparency", Variant::Float32(0.75))
        .unwrap();
    let meshes = Meshes::default().with_kinds(HashMap::from([(pane, Kind::Glass)]));

    let exported = export(&dom, &meshes, pane);
    let document = document(&exported);

    let material = &document["materials"][0];
    assert_eq!(
        material["extensions"]["KHR_materials_transmission"]["transmissionFactor"],
        0.75
    );
    assert_eq!(material["extensions"]["KHR_materials_ior"]["ior"], 1.5);
    assert!(
        material.get("alphaMode").is_none(),
        "transmission, not blending"
    );
    assert_eq!(material["pbrMetallicRoughness"]["baseColorFactor"][3], 1.0);
    let mtl = mtl(&exported.meshes, "x");
    assert!(mtl.contains("Tr 0.75") && mtl.contains("Ni 1.5"));
}

/// A union whose boolean could not be run exports the pieces the viewport
/// draws it as, each in its own colour, rather than its box.
#[test]
fn a_union_drawn_as_its_pieces_exports_them() {
    let mut dom = WeakDom::new();
    let union = part(&mut dom, "UnionOperation", [4.0, 4.0, 4.0]);
    let piece = |at: f32, color: [f32; 3]| Piece {
        kind: ShapeKind::Box,
        transform: Mat4::from_translation(Vec3::new(at, 0.0, 0.0)),
        color,
        alpha: 1.0,
    };
    let meshes = Meshes::default().with_pieces(HashMap::from([(
        union,
        vec![piece(-1.0, [1.0, 0.0, 0.0]), piece(1.0, [0.0, 0.0, 1.0])],
    )]));

    let exported = export(&dom, &meshes, union);

    assert_eq!(exported.meshes.len(), 2);
    assert_eq!(exported.meshes[0].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(exported.meshes[1].color, [0.0, 0.0, 1.0, 1.0]);
    assert!(exported.meshes[0]
        .positions
        .iter()
        .all(|p| p[0] <= -0.5 + 1e-5));
    assert_eq!(exported.nodes[0].meshes, [0, 1]);
}

/// Studio's own export keeps the tree: a whole-place export from
/// `Workspace` nests each part under the models it sits in, each node named
/// and typed after its instance.
#[test]
fn the_instance_tree_comes_along() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Car", Some(workspace));
    let folder = dom.new_instance("Folder", "Body", Some(model));
    let wheel = part(&mut dom, "Part", [1.0, 1.0, 1.0]);
    dom.set_parent(wheel, Some(folder));
    dom.new_instance("Folder", "Empty", Some(workspace));

    let exported = export(&dom, &Meshes::default(), workspace);
    let names: Vec<_> = exported
        .nodes
        .iter()
        .map(|n| (n.name.as_str(), n.parent))
        .collect();

    assert_eq!(
        names,
        [
            ("Workspace", None),
            ("Car", Some(0)),
            ("Body", Some(1)),
            ("Part", Some(2))
        ]
    );
    let document = document(&exported);
    assert_eq!(
        document["nodes"][3]["translation"],
        serde_json::json!([1.0, 2.0, 3.0])
    );
    assert_eq!(document["nodes"][3]["extras"]["Material"], Value::Null);
}

/// A union with no tree, asset or baked mesh anywhere (Studio writes these
/// with a `TriangleCount` of 0) is drawn as nothing, so exports as nothing.
#[test]
fn a_union_with_no_geometry_anywhere_exports_nothing() {
    let mut dom = WeakDom::new();
    let union = part(&mut dom, "UnionOperation", [1.0, 1.0, 1.0]);

    assert!(export(&dom, &Meshes::default(), union).meshes.is_empty());
}

#[path = "force_field_tests.rs"]
mod force_field;

#[path = "budget_tests.rs"]
mod budget;
