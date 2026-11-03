//! `.gltf`/`.glb` geometry: accessors read through the `gltf` crate's reader,
//! with buffers and images found in the file, in data URIs or beside it.

use std::path::Path;

use ::gltf::mesh::Mode;
use ::gltf::{buffer, image, Gltf, Mesh};
use base64::Engine;

use super::{add_side_texture, SourceGeometry, SourceMesh, SourceTexture};

type Buffers = Vec<Option<Vec<u8>>>;

pub(super) fn read(bytes: &[u8], path: &Path) -> Result<SourceGeometry, String> {
    let Gltf { document, blob } =
        Gltf::from_slice(bytes).map_err(|err| format!("Not a valid glTF file: {err}"))?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut out = SourceGeometry::default();

    let buffers: Buffers = document
        .buffers()
        .map(|b| {
            let data = match b.source() {
                buffer::Source::Bin => blob.clone(),
                buffer::Source::Uri(uri) if uri.starts_with("data:") => decode_data(uri),
                buffer::Source::Uri(uri) => std::fs::read(dir.join(decode_uri(uri))).ok(),
            };
            if data.is_none() {
                out.notes.push(format!(
                    "Buffer {} could not be read, so the meshes using it were not checked.",
                    b.index()
                ));
            }
            data
        })
        .collect();

    for mesh in document.meshes() {
        let name = mesh_name(&document, &mesh);
        match read_mesh(&mesh, &buffers, name.clone()) {
            Some(mut m) => {
                m.bones = skin_joints(&document, &mesh);
                out.meshes.push(m);
            }
            None => out.notes.push(format!(
                "The mesh {name} has data that could not be read, so it was not checked."
            )),
        }
    }

    for image in document.images() {
        let embedded = match image.source() {
            image::Source::View { view, .. } => buffers
                .get(view.buffer().index())
                .and_then(|b| b.as_deref())
                .and_then(|b| b.get(view.offset()..view.offset() + view.length()))
                .map(<[u8]>::to_vec),
            image::Source::Uri { uri, .. } if uri.starts_with("data:") => decode_data(uri),
            image::Source::Uri { uri, .. } => {
                add_side_texture(dir, &decode_uri(uri), &mut out);
                continue;
            }
        };
        let size = embedded.and_then(|data| imagesize::blob_size(&data).ok());
        match size {
            Some(size) => out.textures.push(SourceTexture {
                name: format!("image {}", image.index()),
                width: size.width as u32,
                height: size.height as u32,
            }),
            None => out.notes.push(format!(
                "Image {} could not be read, so its size was not checked.",
                image.index()
            )),
        }
    }
    Ok(out)
}

fn mesh_name(document: &::gltf::Document, mesh: &Mesh<'_>) -> String {
    mesh.name()
        .or_else(|| {
            document
                .nodes()
                .find(|n| n.mesh().is_some_and(|m| m.index() == mesh.index()))
                .and_then(|n| n.name())
        })
        .map_or_else(|| format!("mesh {}", mesh.index()), str::to_owned)
}

/// Joint names of the skin on the node that uses `mesh`, if any.
fn skin_joints(document: &::gltf::Document, mesh: &Mesh<'_>) -> Vec<String> {
    document
        .nodes()
        .filter(|n| n.mesh().is_some_and(|m| m.index() == mesh.index()))
        .find_map(|n| n.skin())
        .map(|skin| {
            skin.joints()
                .map(|j| {
                    j.name()
                        .map_or_else(|| format!("node {}", j.index()), str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_mesh(mesh: &Mesh<'_>, buffers: &Buffers, name: String) -> Option<SourceMesh> {
    let mut out = SourceMesh {
        name,
        ..Default::default()
    };
    let (mut has_uvs, mut has_weights) = (false, false);
    for primitive in mesh.primitives() {
        let reader = primitive.reader(|b| buffers.get(b.index()).and_then(|d| d.as_deref()));
        let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
        let count = positions.len();
        let base = out.positions.len() as u32;
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..count as u32).collect(),
        };
        out.triangles.extend(
            triangulate(primitive.mode(), &indices)
                .into_iter()
                .map(|t| t.map(|i| i + base)),
        );

        let uvs: Option<Vec<[f32; 2]>> = reader
            .read_tex_coords(0)
            .map(|t| t.into_f32().collect())
            .filter(|t: &Vec<[f32; 2]>| t.len() == count);
        has_uvs |= uvs.is_some();
        out.uvs.extend(uvs.unwrap_or_else(|| vec![[0.0; 2]; count]));

        let mut influences = vec![0u8; count];
        for set in 0..2 {
            if let Some(weights) = reader.read_weights(set) {
                has_weights = true;
                for (bones, w) in influences.iter_mut().zip(weights.into_f32()) {
                    *bones += w.iter().filter(|x| **x != 0.0).count() as u8;
                }
            }
        }
        out.influences.extend(influences);
        out.positions.extend(positions);
    }
    if !has_uvs {
        out.uvs.clear();
    }
    if !has_weights {
        out.influences.clear();
    }
    Some(out)
}

fn triangulate(mode: Mode, indices: &[u32]) -> Vec<[u32; 3]> {
    match mode {
        Mode::Triangles => indices.as_chunks::<3>().0.to_vec(),
        Mode::TriangleStrip => indices
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
        Mode::TriangleFan => indices
            .windows(2)
            .skip(1)
            .map(|w| [indices[0], w[0], w[1]])
            .collect(),
        _ => Vec::new(),
    }
}

fn decode_data(uri: &str) -> Option<Vec<u8>> {
    let (head, data) = uri.split_once(',')?;
    if !head.contains("base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// glTF URIs are percent-encoded (`%20`).
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
