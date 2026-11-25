//! What scripts ask of a mesh's shape: the faces and attributes around a
//! vertex, who uses an attribute, neighbours, and the surface queries.
//!
//! Lists about a vertex come newest face first, one entry per corner (the
//! EditableMeshQuery golden's `vertex_normals = n3 n3`).

use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Table, UserDataMethods, Value};

use super::data::{Face, MeshData};
use super::geometry::{blend, closest_on_triangle, dot, ray_triangle, sub, V3};
use super::ids::{self, Id, Kind};
use super::{id_arg, read_vector, sequence, vector, LuaEditableMesh};

const ATTRIBUTES: [(&str, Kind); 3] = [
    ("Normal", Kind::Normal),
    ("UV", Kind::Uv),
    ("Color", Kind::Color),
];

fn check(m: &MeshData, kind: Kind, id: f64) -> Result<Id> {
    match kind {
        Kind::Vertex => m.vertices.check(id),
        Kind::Normal => m.normals.check(id),
        Kind::Uv => m.uvs.check(id),
        Kind::Color => m.colors.check(id),
        Kind::Face => m.faces.check(id),
        Kind::Bone => m.bones.check(id),
    }
}

/// The faces on `vertex`, newest first.
pub(super) fn vertex_faces(m: &MeshData, vertex: Id) -> Vec<Id> {
    m.vertex_faces
        .get(&vertex)
        .map_or_else(Vec::new, |f| f.iter().rev().copied().collect())
}

/// The corner of `face` that `vertex` sits on.
pub(super) fn corner(m: &MeshData, vertex: Id, face: Id) -> Result<usize> {
    m.faces
        .get(face)
        .and_then(|f| f.vertices.iter().position(|&v| v == vertex))
        .ok_or_else(|| mlua::Error::runtime("vertexId not found on requested face"))
}

/// (face, corner) for every corner using `id` of `kind`, newest face first.
fn users(m: &MeshData, kind: Kind, id: Id) -> Vec<(Id, &Face, usize)> {
    let faces: Vec<(Id, &Face)> = m.faces.iter().collect();
    faces
        .into_iter()
        .rev()
        .flat_map(|(fid, face)| {
            let slot = face.slot(kind);
            (0..slot.len())
                .filter(move |&c| slot[c] == id)
                .map(move |c| (fid, face, c))
        })
        .collect()
}

fn with_attribute(lua: &Lua, m: &MeshData, kind: Kind, id: Id, vertices: bool) -> Result<Table> {
    sequence(
        lua,
        users(m, kind, id)
            .into_iter()
            .map(|(fid, face, c)| if vertices { face.vertices[c] } else { fid }),
    )
}

/// Each face's triangles for the surface queries: a quad split along 0-2,
/// as `RaycastLocal` reports its corners (the `quad_ray` row).
fn surface(m: &MeshData) -> Vec<(Id, [Id; 3], [V3; 3])> {
    let mut out = Vec::new();
    for (id, face) in m.faces.iter() {
        let splits: &[[usize; 3]] = match face.vertices.len() {
            4 => &[[0, 1, 2], [0, 2, 3]],
            _ => &[[0, 1, 2]],
        };
        for corners in splits {
            let v = corners.map(|c| face.vertices[c]);
            out.push((id, v, v.map(|v| m.position(v))));
        }
    }
    out
}

fn hit(lua: &Lua, (face, v, at, bary): (Id, [Id; 3], V3, V3)) -> Result<MultiValue> {
    (
        face as f64,
        vector(at),
        vector(bary),
        v[0] as f64,
        v[1] as f64,
        v[2] as f64,
    )
        .into_lua_multi(lua)
}

pub(super) fn add<M: UserDataMethods<LuaEditableMesh>>(methods: &mut M) {
    methods.add_method("GetVertexFaces", |lua, this, v: Value| {
        let v = id_arg(&v, 1)?;
        this.read(|m| sequence(lua, vertex_faces(m, m.vertices.check(v)?)))
    });
    for (name, kind) in ATTRIBUTES {
        let plural = if name == "UV" {
            "UVs".to_owned()
        } else {
            format!("{name}s")
        };
        methods.add_method(format!("GetVertex{plural}"), move |lua, this, v: Value| {
            let v = id_arg(&v, 1)?;
            this.read(|m| {
                let v = m.vertices.check(v)?;
                let list: Vec<Id> = vertex_faces(m, v)
                    .into_iter()
                    .filter_map(|f| {
                        let face = m.faces.get(f)?;
                        let c = face.vertices.iter().position(|&x| x == v)?;
                        Some(face.slot(kind)[c])
                    })
                    .collect();
                sequence(lua, list)
            })
        });
        methods.add_method(
            format!("GetVertexFace{name}"),
            move |_, this, (v, f): (Value, Value)| {
                let (v, f) = (id_arg(&v, 1)?, id_arg(&f, 2)?);
                this.read(|m| {
                    let (v, f) = (m.vertices.check(v)?, m.faces.check(f)?);
                    Ok(m.faces.get(f).expect("checked").slot(kind)[corner(m, v, f)?] as f64)
                })
            },
        );
        methods.add_method(
            format!("SetVertexFace{name}"),
            move |_, this, (v, f, id): (Value, Value, Value)| {
                let (v, f, id) = (id_arg(&v, 1)?, id_arg(&f, 2)?, id_arg(&id, 3)?);
                this.edit(|m| {
                    let (v, f) = (m.vertices.check(v)?, m.faces.check(f)?);
                    let id = check(m, kind, id)?;
                    let c = corner(m, v, f)?;
                    m.faces.get_mut(f).expect("checked").slot_mut(kind)[c] = id;
                    Ok(())
                })
            },
        );
        methods.add_method(
            format!("GetFacesWith{name}"),
            move |lua, this, id: Value| {
                let id = id_arg(&id, 1)?;
                this.read(|m| with_attribute(lua, m, kind, check(m, kind, id)?, false))
            },
        );
        methods.add_method(
            format!("GetVerticesWith{name}"),
            move |lua, this, id: Value| {
                let id = id_arg(&id, 1)?;
                this.read(|m| with_attribute(lua, m, kind, check(m, kind, id)?, true))
            },
        );
    }
    methods.add_method("GetFacesWithAttribute", |lua, this, id: Value| {
        let id = id_arg(&id, 1)?;
        // The faces of every vertex the id is or is on (the golden's
        // `faces_with_attribute_color` lists the seam vertex's other face).
        this.read(|m| {
            let vertices: Vec<Id> = match ids::decode(id) {
                Some((Kind::Vertex, ..)) => vec![m.vertices.check(id)?],
                Some((kind @ (Kind::Normal | Kind::Uv | Kind::Color), ..)) => {
                    let id = check(m, kind, id)?;
                    users(m, kind, id)
                        .into_iter()
                        .map(|(_, face, c)| face.vertices[c])
                        .collect()
                }
                _ => return Err(ids::kind_error(Kind::Vertex, id)),
            };
            let mut faces: Vec<Id> = Vec::new();
            for v in vertices {
                for f in vertex_faces(m, v) {
                    if !faces.contains(&f) {
                        faces.push(f);
                    }
                }
            }
            sequence(lua, faces)
        })
    });
    methods.add_method("GetVerticesWithAttribute", |lua, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| match ids::decode(id) {
            Some((Kind::Face, ..)) => {
                let face = m.faces.check(id)?;
                sequence(lua, m.faces.get(face).expect("checked").vertices.clone())
            }
            Some((Kind::Vertex | Kind::Bone, ..)) | None => Err(ids::kind_error(Kind::Face, id)),
            Some((kind, ..)) => with_attribute(lua, m, kind, check(m, kind, id)?, true),
        })
    });
    methods.add_method("GetAdjacentVertices", |lua, this, v: Value| {
        let v = id_arg(&v, 1)?;
        this.read(|m| {
            let v = m.vertices.check(v)?;
            let mut out: Vec<Id> = Vec::new();
            for f in vertex_faces(m, v) {
                let corners = &m.faces.get(f).expect("listed").vertices;
                let n = corners.len();
                for c in (0..n).filter(|&c| corners[c] == v) {
                    for w in [corners[(c + n - 1) % n], corners[(c + 1) % n]] {
                        if w != v && !out.contains(&w) {
                            out.push(w);
                        }
                    }
                }
            }
            sequence(lua, out)
        })
    });
    methods.add_method("GetAdjacentFaces", |lua, this, f: Value| {
        let f = id_arg(&f, 1)?;
        this.read(|m| {
            let f = m.faces.check(f)?;
            let mine = &m.faces.get(f).expect("checked").vertices;
            let shared = |face: &Face| face.vertices.iter().filter(|v| mine.contains(v)).count();
            let faces: Vec<Id> = m
                .faces
                .iter()
                .filter(|(id, face)| *id != f && shared(face) >= 2)
                .map(|(id, _)| id)
                .collect();
            sequence(lua, faces.into_iter().rev())
        })
    });
    methods.add_method("FindClosestPointOnSurface", |lua, this, point: Value| {
        let point = read_vector(&point)?;
        this.read(|m| {
            let best = surface(m)
                .into_iter()
                .map(|(id, v, corners)| {
                    let bary = closest_on_triangle(point, corners);
                    let at = blend(corners, bary);
                    let d = sub(at, point);
                    (dot(d, d), (id, v, at, bary))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match best {
                Some((_, found)) => hit(lua, found),
                None => Err(mlua::Error::runtime("EditableMesh Error: Empty Result")),
            }
        })
    });
    methods.add_method("FindClosestVertex", |_, this, point: Value| {
        let point = read_vector(&point)?;
        this.read(|m| {
            Ok(m.vertices
                .iter()
                .map(|(id, v)| {
                    let d = sub(v.position, point);
                    (dot(d, d), id)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map_or(0.0, |(_, id)| id as f64))
        })
    });
    methods.add_method(
        "FindVerticesWithinSphere",
        |lua, this, (center, radius): (Value, f32)| {
            let center = read_vector(&center)?;
            this.read(|m| {
                let inside: Vec<Id> = m
                    .vertices
                    .iter()
                    .filter(|(_, v)| {
                        let d = sub(v.position, center);
                        dot(d, d) <= radius * radius
                    })
                    .map(|(id, _)| id)
                    .collect();
                sequence(lua, inside)
            })
        },
    );
    methods.add_method(
        "RaycastLocal",
        |lua, this, (origin, direction): (Value, Value)| {
            let (origin, direction) = (read_vector(&origin)?, read_vector(&direction)?);
            this.read(|m| {
                let first = surface(m)
                    .into_iter()
                    .filter_map(|(id, v, corners)| {
                        ray_triangle(origin, direction, corners)
                            .map(|(t, bary)| (t, (id, v, blend(corners, bary), bary)))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                match first {
                    Some((_, found)) => hit(lua, found),
                    None => Ok(MultiValue::new()),
                }
            })
        },
    );
}
