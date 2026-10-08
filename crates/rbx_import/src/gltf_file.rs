//! `.gltf`/`.glb`: triangle counts from accessor sizes, and side files made
//! part of the document.

use std::path::Path;

use base64::Engine;
use gltf::mesh::Mode;
use gltf::{Gltf, Semantic};
use serde_json::Value;

use crate::MeshInfo;

pub(crate) fn meshes(bytes: &[u8]) -> Result<Vec<MeshInfo>, String> {
    let document = Gltf::from_slice(bytes).map_err(|err| format!("not a valid glTF: {err}"))?;
    Ok(document
        .meshes()
        .map(|mesh| MeshInfo {
            name: mesh
                .name()
                .map_or_else(|| format!("Mesh {}", mesh.index()), str::to_owned),
            triangles: mesh.primitives().map(|p| triangles(&p)).sum(),
        })
        .collect())
}

fn triangles(primitive: &gltf::Primitive) -> usize {
    let corners = primitive
        .indices()
        .or_else(|| primitive.get(&Semantic::Positions))
        .map_or(0, |accessor| accessor.count());
    match primitive.mode() {
        Mode::Triangles => corners / 3,
        Mode::TriangleStrip | Mode::TriangleFan => corners.saturating_sub(2),
        _ => 0,
    }
}

/// A `.gltf` whose buffers and images are separate files cannot be a single
/// upload; each becomes a data URI in the document.
pub(crate) fn inline_side_files(bytes: &[u8], dir: Option<&Path>) -> Result<Vec<u8>, String> {
    let mut root: Value =
        serde_json::from_slice(bytes).map_err(|err| format!("not a valid glTF: {err}"))?;
    for (list, fallback) in [
        ("buffers", "application/octet-stream"),
        ("images", "image/png"),
    ] {
        let Some(entries) = root.get_mut(list).and_then(Value::as_array_mut) else {
            continue;
        };
        for entry in entries {
            let Some(uri) = entry.get("uri").and_then(Value::as_str) else {
                continue;
            };
            if uri.starts_with("data:") {
                continue;
            }
            let dir = dir.ok_or("a .gltf with separate files must be imported from disk")?;
            let name = decode_uri(uri);
            let file = std::fs::read(dir.join(&name))
                .map_err(|err| format!("{name} (named by the .gltf): {err}"))?;
            let mime = if list == "images" {
                image_mime(&name)
            } else {
                fallback
            };
            let data = base64::engine::general_purpose::STANDARD.encode(file);
            entry["uri"] = Value::String(format!("data:{mime};base64,{data}"));
        }
    }
    serde_json::to_vec(&root).map_err(|err| err.to_string())
}

fn image_mime(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    }
}

/// `%20` and friends: glTF URIs are percent-encoded.
fn decode_uri(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
