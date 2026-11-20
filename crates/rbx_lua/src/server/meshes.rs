//! `AssetService`'s runtime meshes: `CreateEditableMesh` and
//! `CreateMeshPartAsync`, which turns an `EditableMesh` into a `MeshPart`
//! whose collision geometry the simulation is told about and whose
//! `MeshContent` keeps showing the mesh as it is edited.
//!
//! Ids are plain numbers from one counter, so a vertex id never passes for a
//! face's. Every vertex brings a normal, UV and colour of its own, which the
//! triangles using it start out with (the golden counts four of each for
//! four vertices); a normal nobody set is the faces' own.

use std::any::Any;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mlua::{
    AnyUserData, IntoLua, IntoLuaMulti, Lua, MetaMethod, MultiValue, ObjectLike, Result, Table,
    UserData, UserDataFields, UserDataMethods, Value,
};
use rbx_dom::editable::{self, lock, Editable, MeshSource, Triangles};
use rbx_dom::{Content, Ref, Variant};

use super::Physics;
use crate::ctx::Ctx;
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::content::{LuaContent, Source};
use crate::datatypes::misc::LuaVector2;
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

type V3 = [f32; 3];

#[derive(Clone)]
struct Vertex {
    position: V3,
    normal: u32,
    uv: u32,
    color: u32,
}

#[derive(Clone)]
struct Face {
    vertices: [u32; 3],
    normals: [u32; 3],
    uvs: [u32; 3],
    colors: [u32; 3],
}

#[derive(Clone, Default)]
struct MeshData {
    next: u32,
    vertices: BTreeMap<u32, Vertex>,
    faces: BTreeMap<u32, Face>,
    /// `None`: worked out from the faces using it.
    normals: BTreeMap<u32, Option<V3>>,
    uvs: BTreeMap<u32, [f32; 2]>,
    /// RGB then alpha, the alpha kept to a byte as Roblox does (0.5 reads
    /// back 0.502).
    colors: BTreeMap<u32, [f32; 4]>,
    fixed: bool,
    version: u64,
}

struct SharedMesh(Mutex<MeshData>);

impl MeshSource for SharedMesh {
    fn version(&self) -> u64 {
        lock(&self.0).version
    }

    fn triangles(&self) -> Triangles {
        let mesh = lock(&self.0);
        let mut out = Triangles::default();
        for face in mesh.faces.values() {
            let flat = mesh.face_normal(face);
            for corner in 0..3 {
                out.positions
                    .push(mesh.vertices[&face.vertices[corner]].position);
                out.normals
                    .push(mesh.normal(face.normals[corner]).unwrap_or(flat));
                out.uvs.push(mesh.uvs[&face.uvs[corner]]);
                out.colors.push(mesh.colors[&face.colors[corner]]);
            }
        }
        out
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

#[derive(Clone)]
struct LuaEditableMesh {
    referent: Ref,
    mesh: Arc<SharedMesh>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(v: V3) -> V3 {
    let length = dot(v, v).sqrt();
    match length > 0.0 {
        true => v.map(|c| c / length),
        false => [0.0; 3],
    }
}
fn blend(corners: [V3; 3], bary: V3) -> V3 {
    std::array::from_fn(|i| {
        corners[0][i] * bary[0] + corners[1][i] * bary[1] + corners[2][i] * bary[2]
    })
}

/// The point of triangle `abc` closest to `p`, as barycentric weights of
/// `a`, `b`, `c` (Ericson, Real-Time Collision Detection, 5.1.5).
fn closest_on_triangle(p: V3, [a, b, c]: [V3; 3]) -> V3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [1.0 - v, v, 0.0];
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0.0, 1.0 - w, w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [1.0 - v - w, v, w]
}

/// Where the segment `origin`..`origin + direction` first crosses triangle
/// `abc`, from either side, as (distance fraction, barycentric weights).
fn ray_triangle(origin: V3, direction: V3, [a, b, c]: [V3; 3]) -> Option<(f32, V3)> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    let t = dot(e2, q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&t))
        .then_some((t, [1.0 - u - v, u, v]))
}

fn id_error(kind: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "Unexpected EditableMesh id type. Expected {kind} id, received value that doesn't appear to be an id"
    ))
}

fn check<T>(map: &BTreeMap<u32, T>, id: f64, kind: &str) -> Result<u32> {
    let key = id as u32;
    match id.fract() == 0.0 && map.contains_key(&key) {
        true => Ok(key),
        false => Err(id_error(kind)),
    }
}

fn ids(list: &Table, count: usize) -> Result<Vec<f64>> {
    let ids: Vec<f64> = list.sequence_values().collect::<Result<_>>()?;
    match ids.len() == count {
        true => Ok(ids),
        false => Err(mlua::Error::runtime(format!(
            "Expected {count} ids, got {}",
            ids.len()
        ))),
    }
}

impl MeshData {
    fn id(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    fn corners(&self, face: &Face) -> [V3; 3] {
        face.vertices.map(|v| self.vertices[&v].position)
    }

    fn face_normal(&self, face: &Face) -> V3 {
        let [a, b, c] = self.corners(face);
        normalized(cross(sub(b, a), sub(c, a)))
    }

    /// A normal's value, a computed one averaged over the faces using it.
    fn normal(&self, id: u32) -> Option<V3> {
        if let Some(Some(set)) = self.normals.get(&id) {
            return Some(*set);
        }
        let mut sum = [0.0; 3];
        for face in self.faces.values().filter(|f| f.normals.contains(&id)) {
            let n = self.face_normal(face);
            sum = std::array::from_fn(|i| sum[i] + n[i]);
        }
        (sum != [0.0; 3]).then(|| normalized(sum))
    }

    fn add_vertex(&mut self, position: V3) -> u32 {
        let id = self.id();
        let (normal, uv, color) = (self.id(), self.id(), self.id());
        self.normals.insert(normal, None);
        self.uvs.insert(uv, [0.0, 0.0]);
        self.colors.insert(color, [1.0; 4]);
        self.vertices.insert(
            id,
            Vertex {
                position,
                normal,
                uv,
                color,
            },
        );
        id
    }

    fn bounds(&self) -> (V3, V3) {
        if self.vertices.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in self.vertices.values() {
            for axis in 0..3 {
                min[axis] = min[axis].min(v.position[axis]);
                max[axis] = max[axis].max(v.position[axis]);
            }
        }
        (min, max)
    }

    fn remove_unused(&mut self) -> Vec<u32> {
        let used = |f: fn(&Face) -> [u32; 3], faces: &BTreeMap<u32, Face>, id: u32| {
            faces.values().any(|face| f(face).contains(&id))
        };
        let gone: Vec<u32> = self
            .vertices
            .keys()
            .copied()
            .filter(|&v| !used(|f| f.vertices, &self.faces, v))
            .collect();
        for v in &gone {
            self.vertices.remove(v);
        }
        let held = |pick: fn(&Vertex) -> u32, vertices: &BTreeMap<u32, Vertex>, id: u32| {
            vertices.values().any(|v| pick(v) == id)
        };
        let (faces, vertices) = (&self.faces, &self.vertices);
        self.normals
            .retain(|&id, _| used(|f| f.normals, faces, id) || held(|v| v.normal, vertices, id));
        self.uvs
            .retain(|&id, _| used(|f| f.uvs, faces, id) || held(|v| v.uv, vertices, id));
        self.colors
            .retain(|&id, _| used(|f| f.colors, faces, id) || held(|v| v.color, vertices, id));
        gone
    }
}

fn read_vector(value: &Value) -> Result<V3> {
    match value {
        Value::UserData(data) if data.is::<LuaVector3>() => {
            let v = data.borrow::<LuaVector3>()?.0;
            Ok([v.x, v.y, v.z])
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Vector3",
            other.type_name()
        ))),
    }
}

fn vector(v: V3) -> LuaVector3 {
    LuaVector3::new(v[0], v[1], v[2])
}

fn sequence(lua: &Lua, ids: impl IntoIterator<Item = u32>) -> Result<Table> {
    lua.create_sequence_from(ids)
}

impl LuaEditableMesh {
    fn read<T>(&self, f: impl FnOnce(&MeshData) -> Result<T>) -> Result<T> {
        f(&lock(&self.mesh.0))
    }

    fn edit<T>(&self, f: impl FnOnce(&mut MeshData) -> Result<T>) -> Result<T> {
        let mut mesh = lock(&self.mesh.0);
        let out = f(&mut mesh)?;
        mesh.version += 1;
        editable::touch();
        Ok(out)
    }
}

/// The face attribute lists `Get/SetFace{Normals,UVs,Colors}` work on.
fn face_slot<'a>(face: &'a mut Face, which: &str) -> &'a mut [u32; 3] {
    match which {
        "Normal" => &mut face.normals,
        "UV" => &mut face.uvs,
        _ => &mut face.colors,
    }
}

impl UserData for LuaEditableMesh {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Object");
        fields.add_field_method_get("FixedSize", |_, this| this.read(|m| Ok(m.fixed)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("EditableMesh"));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(referent(&other) == Some(this.referent))
        });
        methods.add_method("AddVertex", |_, this, position: Value| {
            let position = read_vector(&position)?;
            this.edit(|m| Ok(m.add_vertex(position)))
        });
        methods.add_method("AddTriangle", |_, this, (a, b, c): (f64, f64, f64)| {
            this.edit(|m| {
                let mut corners = Vec::with_capacity(3);
                for id in [a, b, c] {
                    let key = id as u32;
                    match m.vertices.get(&key).filter(|_| id.fract() == 0.0) {
                        Some(vertex) => corners.push((key, vertex.clone())),
                        None => {
                            return Err(mlua::Error::runtime(format!(
                                "Unable to add new triangle, vertex id {id} is invalid"
                            )))
                        }
                    }
                }
                let pick = |f: fn(&Vertex) -> u32| std::array::from_fn(|i| f(&corners[i].1));
                let face = Face {
                    vertices: std::array::from_fn(|i| corners[i].0),
                    normals: pick(|v| v.normal),
                    uvs: pick(|v| v.uv),
                    colors: pick(|v| v.color),
                };
                let id = m.id();
                m.faces.insert(id, face);
                Ok(id)
            })
        });
        methods.add_method("RemoveFace", |_, this, face: f64| {
            this.edit(|m| {
                let face = check(&m.faces, face, "Face")?;
                m.faces.remove(&face);
                Ok(())
            })
        });
        methods.add_method("RemoveUnused", |lua, this, ()| {
            let gone = this.edit(|m| Ok(m.remove_unused()))?;
            sequence(lua, gone)
        });
        methods.add_method("GetPosition", |_, this, id: f64| {
            this.read(|m| {
                Ok(vector(
                    m.vertices[&check(&m.vertices, id, "Vertex")?].position,
                ))
            })
        });
        methods.add_method("SetPosition", |_, this, (id, position): (f64, Value)| {
            let position = read_vector(&position)?;
            this.edit(|m| {
                let id = check(&m.vertices, id, "Vertex")?;
                m.vertices.get_mut(&id).expect("checked").position = position;
                Ok(())
            })
        });
        methods.add_method("GetVertices", |lua, this, ()| {
            this.read(|m| sequence(lua, m.vertices.keys().copied()))
        });
        methods.add_method("GetFaces", |lua, this, ()| {
            this.read(|m| sequence(lua, m.faces.keys().copied()))
        });
        methods.add_method("GetNormals", |lua, this, ()| {
            this.read(|m| sequence(lua, m.normals.keys().copied()))
        });
        methods.add_method("GetUVs", |lua, this, ()| {
            this.read(|m| sequence(lua, m.uvs.keys().copied()))
        });
        methods.add_method("GetColors", |lua, this, ()| {
            this.read(|m| sequence(lua, m.colors.keys().copied()))
        });
        methods.add_method("GetFaceVertices", |lua, this, face: f64| {
            this.read(|m| sequence(lua, m.faces[&check(&m.faces, face, "Face")?].vertices))
        });
        for which in ["Normal", "UV", "Color"] {
            methods.add_method(format!("GetFace{which}s"), move |lua, this, face: f64| {
                this.read(|m| {
                    let mut face = m.faces[&check(&m.faces, face, "Face")?].clone();
                    sequence(lua, *face_slot(&mut face, which))
                })
            });
            methods.add_method(
                format!("SetFace{which}s"),
                move |_, this, (face, list): (f64, Table)| {
                    let list = ids(&list, 3)?;
                    this.edit(|m| {
                        let face = check(&m.faces, face, "Face")?;
                        let mut set = [0; 3];
                        for (slot, id) in set.iter_mut().zip(list) {
                            *slot = match which {
                                "Normal" => check(&m.normals, id, which)?,
                                "UV" => check(&m.uvs, id, which)?,
                                _ => check(&m.colors, id, which)?,
                            };
                        }
                        *face_slot(m.faces.get_mut(&face).expect("checked"), which) = set;
                        Ok(())
                    })
                },
            );
        }
        methods.add_method("AddNormal", |_, this, normal: Option<Value>| {
            let normal = normal
                .filter(|n| !n.is_nil())
                .map(|n| read_vector(&n))
                .transpose()?;
            this.edit(|m| {
                let id = m.id();
                m.normals.insert(id, normal);
                Ok(id)
            })
        });
        methods.add_method("GetNormal", |_, this, id: f64| {
            this.read(|m| {
                Ok(vector(
                    m.normal(check(&m.normals, id, "Normal")?)
                        .unwrap_or_default(),
                ))
            })
        });
        methods.add_method("SetNormal", |_, this, (id, normal): (f64, Value)| {
            let normal = read_vector(&normal)?;
            this.edit(|m| {
                let id = check(&m.normals, id, "Normal")?;
                m.normals.insert(id, Some(normal));
                Ok(())
            })
        });
        methods.add_method("AddUV", |_, this, uv: AnyUserData| {
            let uv = uv.borrow::<LuaVector2>()?.0;
            this.edit(|m| {
                let id = m.id();
                m.uvs.insert(id, [uv.x, uv.y]);
                Ok(id)
            })
        });
        methods.add_method("GetUV", |_, this, id: f64| {
            this.read(|m| {
                let [x, y] = m.uvs[&check(&m.uvs, id, "UV")?];
                Ok(LuaVector2::new(x, y))
            })
        });
        methods.add_method("SetUV", |_, this, (id, uv): (f64, AnyUserData)| {
            let uv = uv.borrow::<LuaVector2>()?.0;
            this.edit(|m| {
                let id = check(&m.uvs, id, "UV")?;
                m.uvs.insert(id, [uv.x, uv.y]);
                Ok(())
            })
        });
        let byte = |alpha: f32| (alpha.clamp(0.0, 1.0) * 255.0).round() / 255.0;
        methods.add_method(
            "AddColor",
            move |_, this, (color, alpha): (AnyUserData, f32)| {
                let c = color.borrow::<LuaColor3>()?.0;
                this.edit(|m| {
                    let id = m.id();
                    m.colors.insert(id, [c.r, c.g, c.b, byte(alpha)]);
                    Ok(id)
                })
            },
        );
        methods.add_method("GetColor", |_, this, id: f64| {
            this.read(|m| {
                let [r, g, b, _] = m.colors[&check(&m.colors, id, "Color")?];
                Ok(LuaColor3::new(r, g, b))
            })
        });
        methods.add_method("SetColor", |_, this, (id, color): (f64, AnyUserData)| {
            let c = color.borrow::<LuaColor3>()?.0;
            this.edit(|m| {
                let id = check(&m.colors, id, "Color")?;
                let entry = m.colors.get_mut(&id).expect("checked");
                *entry = [c.r, c.g, c.b, entry[3]];
                Ok(())
            })
        });
        methods.add_method("GetColorAlpha", |_, this, id: f64| {
            this.read(|m| Ok(m.colors[&check(&m.colors, id, "Color")?][3]))
        });
        methods.add_method("SetColorAlpha", move |_, this, (id, alpha): (f64, f32)| {
            this.edit(|m| {
                let id = check(&m.colors, id, "Color")?;
                m.colors.get_mut(&id).expect("checked")[3] = byte(alpha);
                Ok(())
            })
        });
        methods.add_method("GetVertexFaces", |lua, this, id: f64| {
            this.read(|m| {
                let id = check(&m.vertices, id, "Vertex")?;
                sequence(
                    lua,
                    m.faces
                        .iter()
                        .filter(|(_, f)| f.vertices.contains(&id))
                        .map(|(k, _)| *k),
                )
            })
        });
        methods.add_method("GetAdjacentVertices", |lua, this, id: f64| {
            this.read(|m| {
                let id = check(&m.vertices, id, "Vertex")?;
                let mut near: Vec<u32> = m
                    .faces
                    .values()
                    .filter(|f| f.vertices.contains(&id))
                    .flat_map(|f| f.vertices)
                    .filter(|&v| v != id)
                    .collect();
                near.sort_unstable();
                near.dedup();
                sequence(lua, near)
            })
        });
        methods.add_method("GetAdjacentFaces", |lua, this, id: f64| {
            this.read(|m| {
                let face = m.faces[&check(&m.faces, id, "Face")?].vertices;
                let shared = |f: &Face| f.vertices.iter().filter(|v| face.contains(v)).count();
                sequence(
                    lua,
                    m.faces
                        .iter()
                        .filter(|(&k, f)| k != id as u32 && shared(f) >= 2)
                        .map(|(k, _)| *k),
                )
            })
        });
        methods.add_method("FindClosestPointOnSurface", |lua, this, point: Value| {
            let point = read_vector(&point)?;
            this.read(|m| {
                let best = m
                    .faces
                    .iter()
                    .map(|(id, face)| {
                        let corners = m.corners(face);
                        let bary = closest_on_triangle(point, corners);
                        let at = blend(corners, bary);
                        let d = sub(at, point);
                        (dot(d, d), *id, at, bary)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                match best {
                    Some((_, id, at, bary)) => (id, vector(at), vector(bary)).into_lua_multi(lua),
                    None => Ok(MultiValue::new()),
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
                        (dot(d, d), *id)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, id)| id))
            })
        });
        methods.add_method(
            "FindVerticesWithinSphere",
            |lua, this, (center, radius): (Value, f32)| {
                let center = read_vector(&center)?;
                this.read(|m| {
                    sequence(
                        lua,
                        m.vertices
                            .iter()
                            .filter(|(_, v)| {
                                let d = sub(v.position, center);
                                dot(d, d) <= radius * radius
                            })
                            .map(|(id, _)| *id),
                    )
                })
            },
        );
        // Face first: the golden's `local point, face, bary = ...` gets a
        // number for `point`.
        methods.add_method(
            "RaycastLocal",
            |lua, this, (origin, direction): (Value, Value)| {
                let (origin, direction) = (read_vector(&origin)?, read_vector(&direction)?);
                this.read(|m| {
                    let hit = m
                        .faces
                        .iter()
                        .filter_map(|(id, face)| {
                            let corners = m.corners(face);
                            ray_triangle(origin, direction, corners)
                                .map(|(t, bary)| (t, *id, blend(corners, bary), bary))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    match hit {
                        Some((_, id, at, bary)) => {
                            (id, vector(at), vector(bary)).into_lua_multi(lua)
                        }
                        None => Ok(MultiValue::new()),
                    }
                })
            },
        );
        methods.add_method("GetSize", |_, this, ()| {
            this.read(|m| {
                let (min, max) = m.bounds();
                Ok(vector(sub(max, min)))
            })
        });
        methods.add_method("GetCenter", |_, this, ()| {
            this.read(|m| {
                let (min, max) = m.bounds();
                Ok(vector(std::array::from_fn(|i| (min[i] + max[i]) / 2.0)))
            })
        });
        methods.add_method("Destroy", |_, this, ()| {
            editable::withdraw(this.referent);
            *lock(&this.mesh.0) = MeshData::default();
            Ok(())
        });
    }
}

/// The referent an `EditableMesh` handle is published under.
pub(super) fn referent(data: &AnyUserData) -> Option<Ref> {
    data.borrow::<LuaEditableMesh>()
        .ok()
        .map(|mesh| mesh.referent)
}

/// A handle on the published mesh `referent`.
pub(super) fn handle(lua: &Lua, referent: Ref, mesh: Arc<dyn MeshSource>) -> Result<AnyUserData> {
    let mesh = mesh
        .into_any()
        .downcast::<SharedMesh>()
        .map_err(|_| mlua::Error::runtime("not an EditableMesh"))?;
    lua.create_userdata(LuaEditableMesh { referent, mesh })
}

fn new_mesh(lua: &Lua, data: MeshData) -> LuaEditableMesh {
    let mesh = Arc::new(SharedMesh(Mutex::new(data)));
    let referent = super::editable_image::publish(lua, Editable::Mesh(mesh.clone()));
    LuaEditableMesh { referent, mesh }
}

fn editable_mesh(content: &Value) -> Result<LuaEditableMesh> {
    if let Value::UserData(data) = content {
        if let Ok(content) = data.borrow::<LuaContent>() {
            if let Source::Object(Value::UserData(object)) = &content.0 {
                if let Ok(mesh) = object.borrow::<LuaEditableMesh>() {
                    return Ok(mesh.clone());
                }
            }
            return Err(mlua::Error::runtime(
                "CreateMeshPartAsync: Content must hold an EditableMesh",
            ));
        }
    }
    Err(mlua::Error::runtime(format!(
        "Content expected, got {}",
        content.type_name()
    )))
}

fn create_mesh_part(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    content: &Value,
    options: Option<Table>,
) -> Result<Value> {
    let editable = editable_mesh(content)?;
    let (min, max, vertices, triangles) = editable.read(|mesh| {
        if mesh.vertices.is_empty() {
            return Err(mlua::Error::runtime(
                "CreateMeshPartAsync: the mesh has no vertices",
            ));
        }
        let (min, max) = mesh.bounds();
        // Roblox keeps the mesh's own origin as the part's origin (the
        // MeshCollision golden), so the vertices are not recentred.
        let index: BTreeMap<u32, u32> = mesh
            .vertices
            .keys()
            .zip(0..)
            .map(|(&id, i)| (id, i))
            .collect();
        let vertices: Vec<V3> = mesh.vertices.values().map(|v| v.position).collect();
        let triangles: Vec<[u32; 3]> = mesh
            .faces
            .values()
            .map(|f| f.vertices.map(|v| index[&v]))
            .collect();
        Ok((min, max, vertices, triangles))
    })?;
    let referent = ctx.dom_mut().new_instance("MeshPart", "MeshPart", None);
    let part = LuaInstance::new(referent, ctx.clone()).into_lua(lua)?;
    let Value::UserData(instance) = &part else {
        return Err(mlua::Error::runtime("MeshPart could not be created"));
    };
    // A flat mesh still makes a part of Roblox's least thickness.
    instance.set(
        "Size",
        vector(std::array::from_fn(|i| (max[i] - min[i]).max(0.001))),
    )?;
    if let Some(options) = options {
        let fidelity: Value = options.get("CollisionFidelity")?;
        if !fidelity.is_nil() {
            instance.set("CollisionFidelity", fidelity)?;
        }
    }
    // Scripts cannot write `MeshContent`; the engine binds it, so the part
    // draws the mesh as it is edited.
    ctx.dom_mut()
        .set_property(
            referent,
            "MeshContent",
            Variant::Content(Content::Object(editable.referent)),
        )
        .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    physics.register_mesh(referent, vertices, triangles);
    Ok(part)
}

fn fixed_option(options: &Option<Table>, default: bool) -> Result<bool> {
    match options {
        Some(options) => Ok(options.get::<Option<bool>>("FixedSize")?.unwrap_or(default)),
        None => Ok(default),
    }
}

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    key: &str,
) -> Result<Option<Value>> {
    let f = match key {
        // An empty mesh is never fixed, whatever the options ask (the
        // golden's `fixed_mesh`).
        "CreateEditableMesh" => {
            lua.create_function(|lua, _: MultiValue| Ok(new_mesh(lua, MeshData::default())))?
        }
        "CreateEditableMeshAsync" => lua.create_function(
            |lua, (_this, content, options): (Value, Value, Option<Table>)| {
                let source = editable_mesh(&content).map_err(|_| {
                    mlua::Error::runtime(
                        "AssetService:CreateEditableMeshAsync failed to load the mesh",
                    )
                })?;
                let mut data = source.read(|m| Ok(m.clone()))?;
                data.fixed = fixed_option(&options, true)?;
                data.version = 0;
                Ok(new_mesh(lua, data))
            },
        )?,
        "CreateMeshPartAsync" => {
            let ctx = ctx.clone();
            let physics = physics.clone();
            lua.create_function(
                move |lua, (_this, content, options): (Value, Value, Option<Table>)| {
                    create_mesh_part(lua, &ctx, &physics, &content, options)
                },
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
