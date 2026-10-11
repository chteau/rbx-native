//! Reading the geometry of `.gltf`/`.glb`, `.obj` and binary `.fbx` into one
//! [`Mesh`], with every node's transform applied. Materials, skins and
//! animation are ignored: the rigger only needs the surface.

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use fbxcel::low::v7400::AttributeValue;
use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;
use glam::{Mat3, Mat4, Quat, Vec2, Vec3};

use crate::mesh::Mesh;
use crate::orient::Axis;
use crate::Error;

/// A geometry file format [`read_bytes`] understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.gltf` or `.glb`.
    Gltf,
    Obj,
    /// Binary FBX 7.x.
    Fbx,
}

impl Format {
    /// The format of a file name's extension.
    pub fn of(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "gltf" | "glb" => Some(Format::Gltf),
            "obj" => Some(Format::Obj),
            "fbx" => Some(Format::Fbx),
            _ => None,
        }
    }
}

/// Reads a mesh file, by its extension. A `.gltf`'s external buffers are
/// read from beside it.
pub fn read(path: &Path) -> Result<Mesh, Error> {
    let format = Format::of(path).ok_or_else(|| {
        Error::UnsupportedFormat(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_owned(),
        )
    })?;
    let io = |source| Error::Io {
        path: path.display().to_string(),
        source,
    };
    let bytes = std::fs::read(path).map_err(io)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    match format {
        Format::Gltf => gltf(&bytes, &|uri| std::fs::read(dir.join(uri)).ok()),
        Format::Obj => obj(&bytes),
        Format::Fbx => fbx(&bytes),
    }
}

/// Reads mesh bytes of `format`. A `.gltf` must carry its buffers inline
/// (GLB binary chunk or data URIs).
pub fn read_bytes(bytes: &[u8], format: Format) -> Result<Mesh, Error> {
    match format {
        Format::Gltf => gltf(bytes, &|_| None),
        Format::Obj => obj(bytes),
        Format::Fbx => fbx(bytes),
    }
}

fn bad(what: &str, detail: impl std::fmt::Display) -> Error {
    Error::Format(format!("not a valid {what}: {detail}"))
}

fn finish(mut mesh: Mesh) -> Result<Mesh, Error> {
    if mesh.triangles.is_empty() {
        return Err(Error::Empty);
    }
    if mesh.uvs.len() != mesh.positions.len() {
        mesh.uvs.clear();
    }
    if mesh.normals.len() != mesh.positions.len() {
        mesh.normals.clear();
    }
    crate::mesh::drop_debris(&mut mesh);
    Ok(mesh)
}

/// Positions by `matrix`, normals by its inverse transpose, and the winding
/// reversed when the transform mirrors.
fn transformed(mut mesh: Mesh, matrix: Mat4) -> Mesh {
    for p in mesh.positions.iter_mut() {
        *p = matrix.transform_point3(*p);
    }
    let linear = Mat3::from_mat4(matrix);
    let normal = linear.inverse().transpose();
    for n in mesh.normals.iter_mut() {
        *n = (normal * *n).try_normalize().unwrap_or(*n);
    }
    if linear.determinant() < 0. {
        for t in mesh.triangles.iter_mut() {
            t.swap(1, 2);
        }
    }
    mesh
}

fn gltf(bytes: &[u8], external: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Mesh, Error> {
    let file = gltf::Gltf::from_slice(bytes).map_err(|e| bad("glTF", e))?;
    let buffers = file
        .buffers()
        .map(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => file
                .blob
                .clone()
                .ok_or_else(|| bad("glTF", "the GLB has no binary chunk")),
            gltf::buffer::Source::Uri(uri) => {
                if let Some(data) = uri.strip_prefix("data:") {
                    let payload = data
                        .split_once(";base64,")
                        .map(|(_, p)| p)
                        .ok_or_else(|| bad("glTF", "only base64 data URIs are supported"))?;
                    base64::engine::general_purpose::STANDARD
                        .decode(payload)
                        .map_err(|e| bad("glTF", e))
                } else {
                    external(uri).ok_or_else(|| {
                        Error::Format(format!("the glTF\u{2019}s buffer file {uri} is missing"))
                    })
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Mesh {
        // glTF 2.0 is +Y up by definition, and its models face +Z.
        up_hint: Some(Axis::PosY),
        front_hint: Some(Axis::PosZ),
        ..Mesh::default()
    };
    let mut stack: Vec<(gltf::Node, Mat4)> =
        match file.default_scene().or_else(|| file.scenes().next()) {
            Some(scene) => scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect(),
            None => {
                let children: std::collections::HashSet<usize> = file
                    .nodes()
                    .flat_map(|n| n.children().map(|c| c.index()).collect::<Vec<_>>())
                    .collect();
                file.nodes()
                    .filter(|n| !children.contains(&n.index()))
                    .map(|n| (n, Mat4::IDENTITY))
                    .collect()
            }
        };
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<Vec3> = positions.map(Vec3::from).collect();
                let n = positions.len() as u32;
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..n).collect(),
                };
                let piece = Mesh {
                    normals: reader
                        .read_normals()
                        .map(|i| i.map(Vec3::from).collect())
                        .unwrap_or_default(),
                    uvs: reader
                        .read_tex_coords(0)
                        .map(|i| i.into_f32().map(Vec2::from).collect())
                        .unwrap_or_default(),
                    positions,
                    triangles: indices
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .copied()
                        .filter(|t| t.iter().all(|&i| i < n))
                        .collect(),
                    ..Mesh::default()
                };
                out.append(transformed(piece, world));
            }
        }
        stack.extend(node.children().map(|c| (c, world)));
    }
    finish(out)
}

fn obj(bytes: &[u8]) -> Result<Mesh, Error> {
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };
    let (models, _) = tobj::load_obj_buf(&mut std::io::Cursor::new(bytes), &options, |_| {
        Ok(Default::default())
    })
    .map_err(|e| bad(".obj", e))?;
    let mut out = Mesh::default();
    for model in models {
        let m = model.mesh;
        let vec3s = |v: &[f32]| {
            v.as_chunks::<3>()
                .0
                .iter()
                .map(|&c| Vec3::from_array(c))
                .collect()
        };
        let positions: Vec<Vec3> = vec3s(&m.positions);
        let n = positions.len() as u32;
        out.append(Mesh {
            normals: vec3s(&m.normals),
            // OBJ puts the UV origin bottom-left.
            uvs: m
                .texcoords
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| Vec2::new(c[0], 1. - c[1]))
                .collect(),
            positions,
            triangles: m
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .copied()
                .filter(|t| t.iter().all(|&i| i < n))
                .collect(),
            ..Mesh::default()
        });
    }
    finish(out)
}

fn number(value: &AttributeValue) -> Option<f64> {
    match value {
        AttributeValue::F64(v) => Some(*v),
        AttributeValue::F32(v) => Some(f64::from(*v)),
        AttributeValue::I32(v) => Some(f64::from(*v)),
        AttributeValue::I64(v) => Some(*v as f64),
        _ => None,
    }
}

/// `Properties70` as name → numbers.
fn properties(node: NodeHandle<'_>) -> HashMap<String, Vec<f64>> {
    let mut out = HashMap::new();
    for list in node.children_by_name("Properties70") {
        for p in list.children_by_name("P") {
            let attrs = p.attributes();
            let Some(name) = attrs.first().and_then(|a| a.get_string()) else {
                continue;
            };
            out.insert(
                name.to_owned(),
                attrs.iter().skip(4).filter_map(number).collect(),
            );
        }
    }
    out
}

fn vec3(values: Option<&Vec<f64>>) -> Option<Vec3> {
    let v = values?;
    (v.len() >= 3).then(|| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32))
}

/// FBX `EOrder` (0 = XYZ … 5 = ZYX, the first letter applied first).
fn euler(degrees: Vec3, order: i32) -> Quat {
    let r = degrees * (std::f32::consts::PI / 180.);
    let (x, y, z) = (
        Quat::from_rotation_x(r.x),
        Quat::from_rotation_y(r.y),
        Quat::from_rotation_z(r.z),
    );
    match order {
        1 => y * z * x,
        2 => x * z * y,
        3 => z * x * y,
        4 => y * x * z,
        5 => x * y * z,
        _ => z * y * x,
    }
}

fn f64s(node: NodeHandle<'_>, name: &str) -> Option<Vec<f64>> {
    match node.children_by_name(name).next()?.attributes().first()? {
        AttributeValue::ArrF64(v) => Some(v.clone()),
        AttributeValue::ArrF32(v) => Some(v.iter().map(|&x| f64::from(x)).collect()),
        _ => None,
    }
}

fn i32s(node: NodeHandle<'_>, name: &str) -> Option<Vec<i32>> {
    match node.children_by_name(name).next()?.attributes().first()? {
        AttributeValue::ArrI32(v) => Some(v.clone()),
        AttributeValue::ArrI64(v) => Some(v.iter().map(|&x| x as i32).collect()),
        _ => None,
    }
}

fn text(node: NodeHandle<'_>, name: &str) -> String {
    node.children_by_name(name)
        .next()
        .and_then(|n| n.attributes().first()?.get_string().map(str::to_owned))
        .unwrap_or_default()
}

/// One `LayerElementNormal`/`LayerElementUV`: per polygon-vertex values,
/// resolved through its mapping and reference modes.
struct Layer {
    values: Vec<f64>,
    width: usize,
    index: Option<Vec<i32>>,
    by_vertex: bool,
}

impl Layer {
    fn read(
        geometry: NodeHandle<'_>,
        element: &str,
        data: &str,
        index: &str,
        width: usize,
    ) -> Option<Layer> {
        let layer = geometry.children_by_name(element).next()?;
        let values = f64s(layer, data)?;
        let mapping = text(layer, "MappingInformationType");
        let reference = text(layer, "ReferenceInformationType");
        Some(Layer {
            values,
            width,
            index: (reference == "IndexToDirect" || reference == "Index")
                .then(|| i32s(layer, index))
                .flatten(),
            by_vertex: mapping == "ByVertice" || mapping == "ByVertex",
        })
    }

    /// The value at polygon-vertex `corner` whose control point is `vertex`.
    fn get(&self, corner: usize, vertex: usize) -> Option<&[f64]> {
        let mut i = if self.by_vertex { vertex } else { corner };
        if let Some(index) = &self.index {
            i = usize::try_from(*index.get(i)?).ok()?;
        }
        self.values.get(i * self.width..(i + 1) * self.width)
    }
}

struct Model {
    local: Mat4,
}

fn fbx(bytes: &[u8]) -> Result<Mesh, Error> {
    let tree = match AnyTree::from_seekable_reader(std::io::Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => return Err(bad("FBX", "unsupported FBX version")),
        Err(_) => {
            return Err(Error::Format(
                "not a binary FBX (export it as binary, not ASCII)".into(),
            ))
        }
    };
    let root = tree.root();

    let mut hints = (None, None);
    for settings in root.children_by_name("GlobalSettings") {
        let p = properties(settings);
        let axis = |name: &str, sign: &str| {
            let k = *p.get(name)?.first()? as usize;
            let s = p.get(sign).and_then(|v| v.first().copied()).unwrap_or(1.);
            let mut v = Vec3::ZERO;
            *v.as_mut().get_mut(k)? = s as f32;
            Axis::of(v)
        };
        hints = (
            axis("UpAxis", "UpAxisSign"),
            axis("FrontAxis", "FrontAxisSign"),
        );
    }

    let mut models: HashMap<i64, Model> = HashMap::new();
    let mut geometries: HashMap<i64, NodeHandle<'_>> = HashMap::new();
    for objects in root.children_by_name("Objects") {
        for node in objects.children() {
            let Some(id) = node.attributes().first().and_then(|a| a.get_i64()) else {
                continue;
            };
            match node.name() {
                "Model" => {
                    let p = properties(node);
                    let order = p
                        .get("RotationOrder")
                        .and_then(|v| v.first())
                        .map_or(0, |v| *v as i32);
                    let rotation = euler(vec3(p.get("PreRotation")).unwrap_or_default(), 0)
                        * euler(vec3(p.get("Lcl Rotation")).unwrap_or_default(), order)
                        * euler(vec3(p.get("PostRotation")).unwrap_or_default(), 0).conjugate();
                    let local = Mat4::from_scale_rotation_translation(
                        vec3(p.get("Lcl Scaling")).unwrap_or(Vec3::ONE),
                        rotation.normalize(),
                        vec3(p.get("Lcl Translation")).unwrap_or_default(),
                    );
                    let geometric = Mat4::from_scale_rotation_translation(
                        vec3(p.get("GeometricScaling")).unwrap_or(Vec3::ONE),
                        euler(vec3(p.get("GeometricRotation")).unwrap_or_default(), 0),
                        vec3(p.get("GeometricTranslation")).unwrap_or_default(),
                    );
                    models.insert(
                        id,
                        Model {
                            local: local * geometric,
                        },
                    );
                }
                "Geometry" => {
                    geometries.insert(id, node);
                }
                _ => {}
            }
        }
    }
    let mut parent: HashMap<i64, i64> = HashMap::new();
    let mut owner: HashMap<i64, i64> = HashMap::new();
    for connections in root.children_by_name("Connections") {
        for c in connections.children_by_name("C") {
            let a = c.attributes();
            let (Some("OO"), Some(child), Some(to)) = (
                a.first().and_then(|v| v.get_string()),
                a.get(1).and_then(|v| v.get_i64()),
                a.get(2).and_then(|v| v.get_i64()),
            ) else {
                continue;
            };
            if models.contains_key(&child) && models.contains_key(&to) {
                parent.insert(child, to);
            } else if geometries.contains_key(&child) && models.contains_key(&to) {
                owner.insert(child, to);
            }
        }
    }
    // The geometric transform applies to the model's own geometry only, not
    // to its children; nobody exports nested geometric offsets in practice,
    // so the chain simply multiplies the full local matrices.
    let world = |model: i64| {
        let mut m = Mat4::IDENTITY;
        let mut at = Some(model);
        let mut guard = 0;
        while let Some(id) = at {
            let Some(model) = models.get(&id) else { break };
            m = model.local * m;
            at = parent.get(&id).copied();
            guard += 1;
            if guard > 1000 {
                break;
            }
        }
        m
    };

    let mut out = Mesh {
        up_hint: hints.0,
        front_hint: hints.1,
        ..Mesh::default()
    };
    let mut ids: Vec<i64> = geometries.keys().copied().collect();
    ids.sort();
    for id in ids {
        let geometry = geometries[&id];
        let (Some(vertices), Some(polygons)) = (
            f64s(geometry, "Vertices"),
            i32s(geometry, "PolygonVertexIndex"),
        ) else {
            continue;
        };
        let control: Vec<Vec3> = vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32))
            .collect();
        let normals = Layer::read(geometry, "LayerElementNormal", "Normals", "NormalsIndex", 3);
        let uvs = Layer::read(geometry, "LayerElementUV", "UV", "UVIndex", 2);
        let mut piece = Mesh::default();
        let mut polygon: Vec<u32> = Vec::new();
        for (corner, &raw) in polygons.iter().enumerate() {
            let vertex = if raw < 0 { !raw } else { raw } as usize;
            let Some(&p) = control.get(vertex) else {
                return Err(bad("FBX", "a polygon addresses a missing vertex"));
            };
            polygon.push(piece.positions.len() as u32);
            piece.positions.push(p);
            if let Some(n) = normals.as_ref().and_then(|l| l.get(corner, vertex)) {
                piece
                    .normals
                    .push(Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32));
            }
            if let Some(uv) = uvs.as_ref().and_then(|l| l.get(corner, vertex)) {
                // FBX puts the UV origin bottom-left.
                piece.uvs.push(Vec2::new(uv[0] as f32, 1. - uv[1] as f32));
            }
            if raw < 0 {
                for k in 1..polygon.len().saturating_sub(1) {
                    piece
                        .triangles
                        .push([polygon[0], polygon[k], polygon[k + 1]]);
                }
                polygon.clear();
            }
        }
        if piece.normals.len() != piece.positions.len() {
            piece.normals.clear();
        }
        if piece.uvs.len() != piece.positions.len() {
            piece.uvs.clear();
        }
        let matrix = owner.get(&id).map_or(Mat4::IDENTITY, |&m| world(m));
        out.append(transformed(piece, matrix));
    }
    finish(out)
}
