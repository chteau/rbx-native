//! `.gltf`/`.glb`: triangle counts from accessor sizes, and side files made
//! part of the document.

use std::path::Path;

use base64::Engine;
use gltf::buffer::Source;
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

/// Brings every mesh over `limit` triangles under it by replacing its
/// triangle primitives' indices with simplified ones; vertex data stays as is.
pub(crate) fn decimate(
    bytes: &[u8],
    limit: usize,
) -> Result<(Vec<u8>, Vec<crate::Decimated>), String> {
    let glb = bytes.starts_with(b"glTF");
    let parsed = Gltf::from_slice(bytes).map_err(|err| format!("not a valid glTF: {err}"))?;
    let buffers = load_buffers(&parsed)?;
    let mut root: Value = if glb {
        glb_json(bytes)?
    } else {
        serde_json::from_slice(bytes).map_err(|err| format!("not a valid glTF: {err}"))?
    };
    let mut extra = if glb {
        parsed.blob.clone().unwrap_or_default()
    } else {
        Vec::new()
    };
    while extra.len() % 4 != 0 {
        extra.push(0);
    }
    let buffer_index = if glb {
        0
    } else {
        root.get("buffers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };

    let mut decimated = Vec::new();
    for mesh in parsed.meshes() {
        let total: usize = mesh.primitives().map(|p| triangles(&p)).sum();
        if total <= limit {
            continue;
        }
        let name = mesh
            .name()
            .map_or_else(|| format!("Mesh {}", mesh.index()), str::to_owned);
        let mut after = 0;
        for primitive in mesh.primitives() {
            let count = triangles(&primitive);
            let mode = primitive.mode();
            if count == 0
                || !matches!(
                    mode,
                    Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan
                )
            {
                after += count;
                continue;
            }
            let unreadable = || format!("{name}: its vertex data can\u{2019}t be read");
            let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            let positions: Vec<[f32; 3]> =
                reader.read_positions().ok_or_else(unreadable)?.collect();
            let corners: Vec<u32> = if primitive.indices().is_some() {
                reader
                    .read_indices()
                    .ok_or_else(unreadable)?
                    .into_u32()
                    .collect()
            } else {
                (0..positions.len() as u32).collect()
            };
            let list = triangle_list(mode, &corners);
            if list
                .iter()
                .flatten()
                .any(|&i| i as usize >= positions.len())
            {
                return Err(format!("{name}: an index points past its vertices"));
            }
            let target = (limit * count / total).max(1);
            let simplified = crate::decimate::simplify(&positions, &list, target);
            after += simplified.len();

            let offset = extra.len();
            extra.extend(simplified.iter().flatten().flat_map(|i| i.to_le_bytes()));
            let view = push_entry(
                &mut root,
                "bufferViews",
                serde_json::json!({
                    "buffer": buffer_index,
                    "byteOffset": offset,
                    "byteLength": simplified.len() * 12,
                }),
            );
            let accessor = push_entry(
                &mut root,
                "accessors",
                serde_json::json!({
                    "bufferView": view,
                    "componentType": 5125,
                    "count": simplified.len() * 3,
                    "type": "SCALAR",
                }),
            );
            let target_primitive = root
                .get_mut("meshes")
                .and_then(|m| m.get_mut(mesh.index()))
                .and_then(|m| m.get_mut("primitives"))
                .and_then(|p| p.get_mut(primitive.index()))
                .ok_or_else(|| format!("{name}: not found in the document"))?;
            target_primitive["indices"] = Value::from(accessor);
            target_primitive["mode"] = Value::from(4);
        }
        decimated.push(crate::Decimated {
            name,
            from: total,
            to: after,
        });
    }
    if decimated.is_empty() {
        return Ok((bytes.to_vec(), decimated));
    }

    if glb {
        if root
            .get("buffers")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            root["buffers"] = serde_json::json!([{"byteLength": 0}]);
        }
        root["buffers"][0]["byteLength"] = Value::from(extra.len());
        Ok((glb_container(&root, &extra)?, decimated))
    } else {
        let data = base64::engine::general_purpose::STANDARD.encode(&extra);
        push_entry(
            &mut root,
            "buffers",
            serde_json::json!({
                "byteLength": extra.len(),
                "uri": format!("data:application/octet-stream;base64,{data}"),
            }),
        );
        let out = serde_json::to_vec(&root).map_err(|err| err.to_string())?;
        Ok((out, decimated))
    }
}

fn load_buffers(parsed: &Gltf) -> Result<Vec<Vec<u8>>, String> {
    parsed
        .buffers()
        .map(|buffer| match buffer.source() {
            Source::Bin => parsed
                .blob
                .clone()
                .ok_or_else(|| "the glTF names a binary chunk it doesn\u{2019}t have".to_owned()),
            Source::Uri(uri) => {
                let data = uri
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split_once(','))
                    .filter(|(head, _)| head.ends_with(";base64"))
                    .map(|(_, data)| data)
                    .ok_or("a buffer of the glTF is not an embedded data: URI")?;
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|err| format!("a buffer of the glTF is not valid base64: {err}"))
            }
        })
        .collect()
}

fn triangle_list(mode: Mode, corners: &[u32]) -> Vec<[u32; 3]> {
    match mode {
        Mode::TriangleStrip => corners
            .windows(3)
            .enumerate()
            .map(|(i, w)| {
                if i % 2 == 0 {
                    [w[0], w[1], w[2]]
                } else {
                    [w[1], w[0], w[2]]
                }
            })
            .collect(),
        Mode::TriangleFan => corners
            .windows(2)
            .skip(1)
            .map(|w| [corners[0], w[0], w[1]])
            .collect(),
        _ => corners.as_chunks::<3>().0.to_vec(),
    }
}

/// Appends to the root's array `key` (made if missing) and returns the index.
fn push_entry(root: &mut Value, key: &str, entry: Value) -> usize {
    if !root[key].is_array() {
        root[key] = Value::Array(Vec::new());
    }
    let list = root[key].as_array_mut().expect("just made an array");
    list.push(entry);
    list.len() - 1
}

fn read_u32(bytes: &[u8], at: usize) -> Option<usize> {
    let raw = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
}

fn glb_json(bytes: &[u8]) -> Result<Value, String> {
    let length = read_u32(bytes, 12).ok_or("the .glb is cut short")?;
    let chunk = bytes.get(20..20 + length).ok_or("the .glb is cut short")?;
    serde_json::from_slice(chunk).map_err(|err| format!("not a valid glTF: {err}"))
}

fn glb_container(root: &Value, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut json = serde_json::to_vec(root).map_err(|err| err.to_string())?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = u32::try_from(12 + 8 + json.len() + 8 + bin.len())
        .map_err(|_| "the decimated .glb is over 4 GB".to_owned())?;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}
