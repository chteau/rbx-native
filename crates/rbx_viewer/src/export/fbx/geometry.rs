//! One surface as an FBX `Geometry`, with a single material.

use glam::{Mat4, Vec3};

use super::tree::{name_class, properties, Node, Prop};
use crate::export::ExportMesh;

/// `mesh` in the frame `to_local` takes world space to, as the `Geometry`
/// object `id`.
pub(super) fn geometry(id: i64, name: &str, mesh: &ExportMesh, to_local: Mat4) -> Node {
    let mut vertices = Vec::new();
    let mut corners = Vec::new();
    let mut normals = Vec::new();
    let mut tangents = Vec::new();
    let mut binormals = Vec::new();
    // Only a baked normal map comes with its own frame.
    let with_tangents = !mesh.tangents.is_empty();
    let turn = |v: [f32; 3]| {
        to_local
            .transform_vector3(Vec3::from(v))
            .normalize_or(Vec3::Y)
    };
    for &p in &mesh.positions {
        let local = to_local.transform_point3(Vec3::from(p));
        vertices.extend(local.to_array().map(f64::from));
    }
    // Always written, as every surface has a colour map to sample, if only
    // a one-texel swatch. FBX puts V's origin at the bottom, where Roblox
    // has it on top.
    let uvs: Vec<f64> = match mesh.uvs.is_empty() {
        true => vec![0.0; mesh.positions.len() * 2],
        false => mesh
            .uvs
            .iter()
            .flat_map(|uv| [f64::from(uv[0]), 1.0 - f64::from(uv[1])])
            .collect(),
    };
    let triangles = mesh.indices.as_chunks::<3>().0;
    for triangle in triangles {
        for (corner, &index) in triangle.iter().enumerate() {
            let point = index as i32;
            // The last corner of a polygon is stored as `-index - 1`.
            corners.push(if corner == 2 { !point } else { point });
            let normal = turn(mesh.normals[index as usize]);
            normals.extend(normal.to_array().map(f64::from));
            if with_tangents {
                let t = mesh.tangents[index as usize];
                let tangent = turn([t[0], t[1], t[2]]);
                let binormal = normal.cross(tangent) * t[3];
                tangents.extend(tangent.to_array().map(f64::from));
                binormals.extend(binormal.to_array().map(f64::from));
            }
        }
    }
    let materials = vec![0i32; triangles.len()];
    let uv_index: Vec<i32> = corners
        .iter()
        .map(|&c| if c < 0 { !c } else { c })
        .collect();

    let mut layer = Node::new("Layer", vec![0.into()]).with(Node::value("Version", 100));
    let mut node = Node::new(
        "Geometry",
        vec![
            id.into(),
            name_class(name, "Geometry").into(),
            "Mesh".into(),
        ],
    )
    .with(properties(Vec::new()))
    .with(Node::value("GeometryVersion", 124))
    .with(Node::new("Vertices", vec![Prop::F64s(vertices)]))
    .with(Node::new("PolygonVertexIndex", vec![Prop::I32s(corners)]));
    let mut add =
        |element: &'static str, version: i32, mapping: &str, reference: &str, data: Vec<Node>| {
            let mut child = Node::new(element, vec![0.into()])
                .with(Node::value("Version", version))
                .with(Node::value(
                    "Name",
                    if element == "LayerElementUV" {
                        "UVMap"
                    } else {
                        ""
                    },
                ))
                .with(Node::value("MappingInformationType", mapping))
                .with(Node::value("ReferenceInformationType", reference));
            for entry in data {
                child.push(entry);
            }
            node.push(child);
            layer.push(
                Node::new("LayerElement", Vec::new())
                    .with(Node::value("Type", element))
                    .with(Node::value("TypedIndex", 0)),
            );
        };
    add(
        "LayerElementNormal",
        102,
        "ByPolygonVertex",
        "Direct",
        vec![Node::new("Normals", vec![Prop::F64s(normals)])],
    );
    if with_tangents {
        add(
            "LayerElementBinormal",
            102,
            "ByPolygonVertex",
            "Direct",
            vec![Node::new("Binormals", vec![Prop::F64s(binormals)])],
        );
        add(
            "LayerElementTangent",
            102,
            "ByPolygonVertex",
            "Direct",
            vec![Node::new("Tangents", vec![Prop::F64s(tangents)])],
        );
    }
    add(
        "LayerElementUV",
        101,
        "ByPolygonVertex",
        "IndexToDirect",
        vec![
            Node::new("UV", vec![Prop::F64s(uvs)]),
            Node::new("UVIndex", vec![Prop::I32s(uv_index)]),
        ],
    );
    add(
        "LayerElementMaterial",
        101,
        "ByPolygon",
        "IndexToDirect",
        vec![Node::new("Materials", vec![Prop::I32s(materials)])],
    );
    node.with(layer)
}
