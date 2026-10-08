//! `.obj` rewritten as one-buffer glTF: a mesh per object/group, positions,
//! normals and UVs. Materials and textures are not carried over; Roblox's
//! `.mtl` is not part of the upload either, so the part comes in untextured.

use std::io::Cursor;

use base64::Engine;
use serde_json::{json, Value};

use crate::MeshInfo;

pub(crate) fn convert(bytes: &[u8]) -> Result<(Vec<MeshInfo>, Vec<u8>), String> {
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };
    let (models, _) =
        tobj::load_obj_buf(
            &mut Cursor::new(bytes),
            &options,
            |_| Ok(Default::default()),
        )
        .map_err(|err| format!("not a valid .obj: {err}"))?;

    let mut buffer = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes) = (vec![], vec![], vec![], vec![]);
    let mut infos = Vec::new();
    for model in models.iter().filter(|m| !m.mesh.indices.is_empty()) {
        let mesh = &model.mesh;
        let vertices = mesh.positions.len() / 3;
        let mut attributes = serde_json::Map::new();

        let (min, max) = bounds(&mesh.positions);
        attributes.insert(
            "POSITION".into(),
            push(
                &mut buffer,
                &mut views,
                &mut accessors,
                floats(&mesh.positions),
                5126,
                vertices,
                "VEC3",
                Some((min, max)),
            ),
        );
        if mesh.normals.len() == mesh.positions.len() {
            attributes.insert(
                "NORMAL".into(),
                push(
                    &mut buffer,
                    &mut views,
                    &mut accessors,
                    floats(&mesh.normals),
                    5126,
                    vertices,
                    "VEC3",
                    None,
                ),
            );
        }
        if mesh.texcoords.len() / 2 == vertices {
            // OBJ's V runs up from the bottom edge, glTF's down from the top.
            let flipped: Vec<f32> = mesh
                .texcoords
                .chunks(2)
                .flat_map(|uv| [uv[0], 1.0 - uv[1]])
                .collect();
            attributes.insert(
                "TEXCOORD_0".into(),
                push(
                    &mut buffer,
                    &mut views,
                    &mut accessors,
                    floats(&flipped),
                    5126,
                    vertices,
                    "VEC2",
                    None,
                ),
            );
        }
        let indices = push(
            &mut buffer,
            &mut views,
            &mut accessors,
            mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect(),
            5125,
            mesh.indices.len(),
            "SCALAR",
            None,
        );
        meshes.push(json!({
            "name": model.name,
            "primitives": [{"attributes": attributes, "indices": indices, "mode": 4}],
        }));
        nodes.push(json!({"name": model.name, "mesh": meshes.len() - 1}));
        infos.push(MeshInfo {
            name: model.name.clone(),
            triangles: mesh.indices.len() / 3,
        });
    }
    if infos.is_empty() {
        return Err("the .obj holds no faces".into());
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&buffer);
    let document = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{
            "byteLength": buffer.len(),
            "uri": format!("data:application/octet-stream;base64,{data}"),
        }],
    });
    Ok((
        infos,
        serde_json::to_vec(&document).map_err(|e| e.to_string())?,
    ))
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
    positions.chunks(3).fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(mut lo, mut hi), p| {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
            (lo, hi)
        },
    )
}

/// Appends `data` to the buffer as a view plus an accessor over it and
/// returns the accessor's index. Every blob here is a multiple of 4 bytes,
/// so no padding is needed between views.
#[allow(clippy::too_many_arguments)]
fn push(
    buffer: &mut Vec<u8>,
    views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    data: Vec<u8>,
    component: u32,
    count: usize,
    kind: &str,
    range: Option<([f32; 3], [f32; 3])>,
) -> Value {
    views.push(json!({"buffer": 0, "byteOffset": buffer.len(), "byteLength": data.len()}));
    buffer.extend(data);
    let mut accessor = json!({
        "bufferView": views.len() - 1,
        "componentType": component,
        "count": count,
        "type": kind,
    });
    if let Some((min, max)) = range {
        accessor["min"] = json!(min);
        accessor["max"] = json!(max);
    }
    accessors.push(accessor);
    json!(accessors.len() - 1)
}
