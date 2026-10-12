use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Variant, Vector3Data};
use serde_json::Value;

use super::*;
use crate::pick::{Pack, Surface};
use crate::scene::{union_fit, AlphaMode};

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

mod geometry;
mod images;
mod real;

fn vector3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn part(dom: &mut WeakDom, class: &str, name: &str, parent: Option<Ref>, at: [f32; 3]) -> Ref {
    let referent = dom.new_instance(class, name, parent);
    let [x, y, z] = at;
    dom.set_property(
        referent,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY,
        }),
    )
    .unwrap();
    dom.set_property(referent, "size", vector3(1.0, 1.0, 1.0))
        .unwrap();
    referent
}

fn export(dom: &WeakDom, meshes: &Meshes, roots: &[Ref]) -> Export {
    meshes_of(dom, &ReflectionDatabase::embedded(), meshes, roots)
}

/// One triangle two units across, its UVs spanning the image.
fn triangle() -> rbx_mesh::SkinnedMesh {
    let vertex = |position: [f32; 3], uv: [f32; 2]| rbx_mesh::Vertex {
        position,
        normal: [0.0, 0.0, 1.0],
        uv,
        color: [255; 4],
    };
    rbx_mesh::SkinnedMesh::from(rbx_mesh::Mesh {
        version: (4, 1),
        vertices: vec![
            vertex([-1.0, -1.0, 0.0], [0.0, 1.0]),
            vertex([1.0, -1.0, 0.0], [1.0, 1.0]),
            vertex([0.0, 1.0, 0.0], [0.5, 0.0]),
        ],
        indices: vec![0, 1, 2],
        lods: Vec::new(),
        bounds: rbx_mesh::Aabb {
            min: [-1.0, -1.0, 0.0],
            max: [1.0, 1.0, 0.0],
        },
    })
}

fn count(text: &str, prefix: &str) -> usize {
    text.lines().filter(|line| line.starts_with(prefix)).count()
}

fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut pixels).unwrap();
    (info.width, info.height, pixels)
}

/// The PNG a glTF texture index embeds.
fn embedded(document: &Value, texture: &Value) -> Vec<u8> {
    let source = &document["textures"][texture["index"].as_u64().unwrap() as usize]["source"];
    let uri = document["images"][source.as_u64().unwrap() as usize]["uri"]
        .as_str()
        .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(uri.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap()
}

fn image(width: u32, height: u32, pixels: &[u8]) -> Arc<Image> {
    Arc::new(Image {
        width,
        height,
        pixels: pixels.to_vec(),
    })
}

/// A red `MeshPart` wearing a `SurfaceAppearance` with `alpha_mode`, a
/// two-texel colour map (clear blue, then opaque green), a normal map, a
/// one-texel metalness map and a two-texel roughness map.
fn dressed(alpha_mode: AlphaMode) -> (Export, [Arc<Image>; 4]) {
    let mut dom = WeakDom::new();
    let statue = part(&mut dom, "MeshPart", "Statue", None, [0.0, 0.0, 0.0]);
    dom.set_property(statue, "MeshId", Variant::String("rbxassetid://42".into()))
        .unwrap();
    dom.set_property(
        statue,
        "Color3uint8",
        Variant::Color3uint8 { r: 255, g: 0, b: 0 },
    )
    .unwrap();
    let maps = [
        image(2, 1, &[0, 0, 255, 0, 0, 255, 0, 255]),
        image(1, 1, &[128, 128, 255, 255]),
        image(1, 1, &[200, 0, 0, 255]),
        image(2, 1, &[10, 0, 0, 255, 90, 0, 0, 255]),
    ];
    let meshes = Meshes::new(
        HashMap::from([(AssetRef::Id(42), Arc::new(triangle()))]),
        HashMap::new(),
    )
    .with_surfaces(HashMap::from([(
        statue,
        Surface {
            maps: maps.clone().map(Some),
            tint: [0.5, 1.0, 1.0],
            alpha_mode,
        },
    )]));
    (export(&dom, &meshes, &[statue]), maps)
}
