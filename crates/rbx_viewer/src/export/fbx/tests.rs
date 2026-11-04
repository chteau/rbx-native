use std::io::Cursor;

use fbxcel::low::v7400::AttributeValue;
use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::{NodeHandle, Tree};
use glam::{Quat, Vec3};

use super::fbx;
use crate::export::{Export, ExportMesh, ExportNode, Finish, Maps};

/// A one-triangle surface at `at`, world space.
fn surface(name: &str, at: [f32; 3], uvs: bool) -> ExportMesh {
    let [x, y, z] = at;
    ExportMesh {
        name: name.into(),
        positions: vec![[x, y, z], [x + 1.0, y, z], [x, y + 1.0, z]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        tangents: Vec::new(),
        indices: vec![0, 1, 2],
        color: [1.0, 0.5, 0.25, 1.0],
        uvs: if uvs {
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 0.25]]
        } else {
            Vec::new()
        },
        maps: Maps::default(),
        blend: false,
        finish: Finish::Plain,
    }
}

/// A `Model` holding `Block` (turned a quarter about Y, at 10,2,0, drawn
/// as a textured surface and a plain one) and a Neon `Lamp`.
fn sample() -> Export {
    let mut textured = surface("Block", [10.0, 2.0, 0.0], true);
    textured.maps.color = Some(0);
    let mut lamp = surface("Lamp", [0.0, 5.0, 0.0], false);
    lamp.finish = Finish::Neon;
    let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array();
    Export {
        meshes: vec![textured, surface("Block", [10.0, 2.0, 0.0], false), lamp],
        textures: vec![b"\x89PNG not really".to_vec()],
        nodes: vec![
            ExportNode {
                name: "Model".into(),
                class: "Model".into(),
                parent: None,
                placement: None,
                meshes: Vec::new(),
                material: None,
            },
            ExportNode {
                name: "Block".into(),
                class: "Part".into(),
                parent: Some(0),
                placement: Some((quarter, [10.0, 2.0, 0.0])),
                meshes: vec![0, 1],
                material: Some("Plastic".into()),
            },
            ExportNode {
                name: "Lamp".into(),
                class: "Part".into(),
                parent: Some(0),
                placement: Some((Quat::IDENTITY.to_array(), [0.0, 5.0, 0.0])),
                meshes: vec![2],
                material: Some("Neon".into()),
            },
        ],
    }
}

fn parse(bytes: &[u8]) -> Tree {
    match AnyTree::from_seekable_reader(Cursor::new(bytes)).expect("parses") {
        AnyTree::V7400(version, tree, _) => {
            assert_eq!(version, fbxcel::low::FbxVersion::V7_4);
            tree
        }
        _ => panic!("not FBX 7.4"),
    }
}

fn objects<'a>(tree: &'a Tree, kind: &str) -> Vec<NodeHandle<'a>> {
    let root = tree.root();
    let objects = root.first_child_by_name("Objects").expect("Objects");
    objects.children_by_name(kind).collect()
}

fn child<'a>(node: NodeHandle<'a>, name: &str) -> NodeHandle<'a> {
    node.first_child_by_name(name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn f64s(node: NodeHandle<'_>) -> Vec<f64> {
    match &node.attributes()[0] {
        AttributeValue::ArrF64(values) => values.clone(),
        other => panic!("not doubles: {other:?}"),
    }
}

fn i32s(node: NodeHandle<'_>) -> Vec<i32> {
    match &node.attributes()[0] {
        AttributeValue::ArrI32(values) => values.clone(),
        other => panic!("not ints: {other:?}"),
    }
}

/// The values of the `P` entry `name` in `node`'s `Properties70`.
fn property(node: NodeHandle<'_>, name: &str) -> Vec<f64> {
    child(node, "Properties70")
        .children_by_name("P")
        .find(|p| p.attributes()[0].get_string() == Some(name))
        .unwrap_or_else(|| panic!("no {name}"))
        .attributes()[4..]
        .iter()
        .filter_map(AttributeValue::get_f64)
        .collect()
}

fn connections(tree: &Tree) -> Vec<(i64, i64)> {
    let root = tree.root();
    let connections = root
        .first_child_by_name("Connections")
        .expect("Connections");
    connections
        .children_by_name("C")
        .map(|c| {
            (
                c.attributes()[1].get_i64().unwrap(),
                c.attributes()[2].get_i64().unwrap(),
            )
        })
        .collect()
}

fn id(node: NodeHandle<'_>) -> i64 {
    node.attributes()[0].get_i64().unwrap()
}

fn name(node: NodeHandle<'_>) -> &str {
    node.attributes()[1]
        .get_string()
        .unwrap()
        .split('\0')
        .next()
        .unwrap()
}

#[test]
fn the_tree_comes_along_placed_relative_to_parents() {
    let tree = parse(&fbx(&sample()).unwrap());
    let models = objects(&tree, "Model");
    let names: Vec<&str> = models.iter().map(|&m| name(m)).collect();
    assert_eq!(names, ["Model", "Block", "Lamp"]);
    assert_eq!(models[0].attributes()[2].get_string(), Some("Null"));
    assert_eq!(models[1].attributes()[2].get_string(), Some("Mesh"));
    let links = connections(&tree);
    assert!(links.contains(&(id(models[0]), 0)));
    assert!(links.contains(&(id(models[1]), id(models[0]))));
    let rotation = property(models[1], "Lcl Rotation");
    assert!(
        (rotation[1] - 90.0).abs() < 1e-3 && rotation[0].abs() < 1e-3,
        "{rotation:?}"
    );
    assert_eq!(property(models[1], "Lcl Translation"), [10.0, 2.0, 0.0]);
}

#[test]
fn a_part_is_one_geometry_in_its_own_frame_with_a_material_per_surface() {
    let tree = parse(&fbx(&sample()).unwrap());
    let geometries = objects(&tree, "Geometry");
    assert_eq!(geometries.len(), 2);
    let block = geometries[0];
    let corners = i32s(child(block, "PolygonVertexIndex"));
    assert_eq!(corners, [0, 1, !2, 3, 4, !5]);
    // The second corner sits one stud along world X, which the quarter
    // turn about Y makes local +Z.
    let vertices = f64s(child(block, "Vertices"));
    let second = Vec3::new(vertices[3] as f32, vertices[4] as f32, vertices[5] as f32);
    assert!(second.abs_diff_eq(Vec3::Z, 1e-5), "{second}");
    let materials = i32s(child(child(block, "LayerElementMaterial"), "Materials"));
    assert_eq!(materials, [0, 1]);
    // V flipped, the plain surface's corners at the origin.
    let uvs = f64s(child(child(block, "LayerElementUV"), "UV"));
    assert_eq!(uvs[..6], [0.0, 1.0, 1.0, 1.0, 0.0, 0.75]);
    assert_eq!(uvs[6..], [0.0; 6]);
    let model = objects(&tree, "Model")[1];
    let mats = objects(&tree, "Material");
    let links = connections(&tree);
    let attached: Vec<i64> = links
        .iter()
        .filter(|(child, parent)| *parent == id(model) && mats.iter().any(|m| id(*m) == *child))
        .map(|(child, _)| *child)
        .collect();
    assert_eq!(attached, [id(mats[0]), id(mats[1])]);
    assert!(links.contains(&(id(block), id(model))));
}

#[test]
fn materials_carry_colour_neon_and_the_embedded_image() {
    let tree = parse(&fbx(&sample()).unwrap());
    let materials = objects(&tree, "Material");
    assert_eq!(property(materials[0], "DiffuseColor"), [1.0, 0.5, 0.25]);
    assert_eq!(property(materials[0], "EmissiveFactor"), [0.0]);
    assert_eq!(property(materials[2], "EmissiveColor"), [1.0, 0.5, 0.25]);
    let videos = objects(&tree, "Video");
    assert_eq!(videos.len(), 1);
    let content = child(videos[0], "Content");
    assert_eq!(
        content.attributes()[0].get_binary(),
        Some(&b"\x89PNG not really"[..])
    );
    let texture = objects(&tree, "Texture")[0];
    let links = connections(&tree);
    assert!(links.contains(&(id(videos[0]), id(texture))));
    assert!(links.contains(&(id(texture), id(materials[0]))));
}

#[test]
fn the_same_export_writes_the_same_bytes() {
    assert_eq!(fbx(&sample()).unwrap(), fbx(&sample()).unwrap());
}

/// A 1x1 RGBA PNG of `texel`.
fn png_of(texel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&texel).unwrap();
    writer.finish().unwrap();
    bytes
}

/// The embedded image of the texture on material `material`, decoded.
fn image_on(tree: &Tree, material: usize) -> (png::ColorType, Vec<u8>) {
    let links = connections(tree);
    let target = id(objects(tree, "Material")[material]);
    let texture = objects(tree, "Texture")
        .into_iter()
        .find(|t| links.contains(&(id(*t), target)))
        .expect("a texture on the material");
    let video = objects(tree, "Video")
        .into_iter()
        .find(|v| links.contains(&(id(*v), id(texture))))
        .expect("a clip under the texture");
    let bytes = child(video, "Content").attributes()[0]
        .get_binary()
        .unwrap()
        .to_vec();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    buffer.truncate(info.buffer_size());
    (info.color_type, buffer)
}

#[test]
fn colour_maps_carry_the_tint_and_only_an_images_own_alpha() {
    let mut export = sample();
    // A grey pack texel with a stray mask in its alpha.
    export.textures = vec![png_of([188, 188, 188, 40])];
    // Surface 1: the same texture on an opaque decal-like surface that
    // blends because of its image, untinted.
    export.meshes[1].maps.color = Some(0);
    export.meshes[1].color = [1.0; 4];
    export.meshes[1].blend = true;
    export.meshes[1].uvs = export.meshes[0].uvs.clone();
    let tree = parse(&fbx(&export).unwrap());
    // Two files: the tinted one and the untouched one.
    assert_eq!(objects(&tree, "Video").len(), 2);

    let (kind, texels) = image_on(&tree, 0);
    assert_eq!(kind, png::ColorType::Rgb);
    // 188 is 0.5 linear: times (1, 0.5, 0.25) and back to sRGB.
    assert_eq!(texels, [188, 137, 99]);

    let (kind, texels) = image_on(&tree, 1);
    assert_eq!(kind, png::ColorType::Rgba);
    assert_eq!(texels, [188, 188, 188, 40]);
}
