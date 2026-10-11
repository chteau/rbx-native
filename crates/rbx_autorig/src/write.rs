//! Output formats: Roblox's `version 2.00` mesh, for previewing a part
//! locally before upload, and one `.glb` of all 15 parts for Open Cloud's
//! model import.

use glam::{Vec2, Vec3};
use serde_json::{json, Value};

use crate::mesh::Mesh;
use crate::split::Body;

/// The mesh as a Roblox `version 2.00` file: a 12-byte header, 40-byte
/// vertices (position, normal, UV, an absent tangent, opaque white) and
/// 12-byte faces, little-endian. Missing normals are written as +Y and
/// missing UVs as zero.
pub fn mesh_v2(mesh: &Mesh) -> Vec<u8> {
    let n = mesh.positions.len();
    let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
    let mut out = Vec::with_capacity(13 + 12 + n * 40 + triangles.len() * 12);
    out.extend_from_slice(b"version 2.00\n");
    out.extend_from_slice(&12u16.to_le_bytes());
    out.push(40);
    out.push(12);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for i in 0..n {
        let p = mesh.positions[i];
        let normal = mesh.normals.get(i).copied().unwrap_or(Vec3::Y);
        let uv = mesh.uvs.get(i).copied().unwrap_or(Vec2::ZERO);
        for v in [p.x, p.y, p.z, normal.x, normal.y, normal.z, uv.x, uv.y] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&[255; 4]);
    }
    for t in triangles {
        for i in t {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    out
}

/// Appends `bytes` to the binary buffer as a buffer view (4-byte aligned)
/// and an accessor; returns the accessor index.
#[allow(clippy::too_many_arguments)]
fn accessor(
    bin: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    bytes: &[u8],
    component: u32,
    count: usize,
    kind: &str,
    target: u32,
    bounds: Option<(Vec3, Vec3)>,
) -> usize {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    views.push(json!({
        "buffer": 0,
        "byteOffset": bin.len(),
        "byteLength": bytes.len(),
        "target": target,
    }));
    bin.extend_from_slice(bytes);
    let mut a = json!({
        "bufferView": views.len() - 1,
        "componentType": component,
        "count": count,
        "type": kind,
    });
    if let Some((lo, hi)) = bounds {
        a["min"] = json!([lo.x, lo.y, lo.z]);
        a["max"] = json!([hi.x, hi.y, hi.z]);
    }
    accessors.push(a);
    accessors.len() - 1
}

fn floats<const N: usize>(items: impl Iterator<Item = [f32; N]>) -> Vec<u8> {
    items.flatten().flat_map(f32::to_le_bytes).collect()
}

/// All 15 parts as one binary glTF: a node per part named `<Part>_Geo` (the
/// importer's naming for body parts) at the part's centre, holding a mesh of
/// the same name. Coordinates are Roblox space as they are: +Y up, the
/// character facing −Z, in studs. Whether Roblox's importer reads glTF
/// units as studs or metres is unverified: read the uploaded parts'
/// `InitialSize` back and compare it with [`crate::PartMesh::size`].
pub fn gltf(body: &Body) -> Vec<u8> {
    let (mut bin, mut views, mut accessors) = (Vec::new(), Vec::new(), Vec::new());
    let (mut meshes, mut nodes) = (Vec::new(), Vec::new());
    for piece in &body.parts {
        let mesh = &piece.mesh;
        let n = mesh.positions.len();
        let name = format!("{}_Geo", piece.part.name());
        let bounds = crate::mesh::bounds(mesh.positions.iter().copied());
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "POSITION".into(),
            json!(accessor(
                &mut bin,
                &mut views,
                &mut accessors,
                &floats(mesh.positions.iter().map(|p| p.to_array())),
                5126,
                n,
                "VEC3",
                34962,
                Some(bounds),
            )),
        );
        if mesh.normals.len() == n {
            attributes.insert(
                "NORMAL".into(),
                json!(accessor(
                    &mut bin,
                    &mut views,
                    &mut accessors,
                    &floats(mesh.normals.iter().map(|p| p.to_array())),
                    5126,
                    n,
                    "VEC3",
                    34962,
                    None,
                )),
            );
        }
        if mesh.uvs.len() == n {
            attributes.insert(
                "TEXCOORD_0".into(),
                json!(accessor(
                    &mut bin,
                    &mut views,
                    &mut accessors,
                    &floats(mesh.uvs.iter().map(|p| p.to_array())),
                    5126,
                    n,
                    "VEC2",
                    34962,
                    None,
                )),
            );
        }
        let indices: Vec<u8> = mesh
            .valid_triangles()
            .flatten()
            .flat_map(u32::to_le_bytes)
            .collect();
        let indices = accessor(
            &mut bin,
            &mut views,
            &mut accessors,
            &indices,
            5125,
            indices.len() / 4,
            "SCALAR",
            34963,
            None,
        );
        meshes.push(json!({
            "name": name,
            "primitives": [{ "attributes": attributes, "indices": indices, "mode": 4 }],
        }));
        nodes.push(json!({
            "name": name,
            "mesh": meshes.len() - 1,
            "translation": piece.centre.to_array(),
        }));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let document = json!({
        "asset": { "version": "2.0", "generator": "rbx-native autorig" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    let mut text = serde_json::to_vec(&document).expect("JSON values always serialise");
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}
