//! A mesh asset's file as an `EditableMesh`: every file vertex becomes a
//! vertex with a normal, UV and colour of its own, in file order, and each
//! triangle of the finest level of detail a face (the EditableAssets
//! golden's `owned_counts = 32 32 32 32 32`). Positions stay where the
//! file has them.

use super::data::{Face, MeshData};

pub(crate) fn from_file(bytes: &[u8]) -> Option<MeshData> {
    let mesh = rbx_mesh::parse(bytes).ok()?;
    let mut data = MeshData::default();
    let mut corners = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        let vertex = data.add_vertex(v.position).ok()?;
        let color = v.color.map(|c| f32::from(c) / 255.0);
        corners.push([
            vertex,
            data.normals.insert(Some(v.normal)),
            data.uvs.insert(v.uv),
            data.colors.insert(color),
        ]);
    }
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let pick =
            |i: usize| -> Vec<u64> { triangle.iter().map(|&t| corners[t as usize][i]).collect() };
        data.put_face(Face {
            vertices: pick(0),
            normals: pick(1),
            uvs: pick(2),
            colors: pick(3),
        });
    }
    Some(data)
}
