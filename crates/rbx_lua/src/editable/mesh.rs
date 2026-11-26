//! `EditableMesh`: the handle scripts hold, the published copy a renderer
//! reads (`rbx_dom::editable`), and the argument helpers its methods share.
//! What the methods do lives in the submodules, by family.

mod batch;
mod bones;
pub(crate) mod data;
mod geometry;
pub(crate) mod ids;
pub(crate) mod load;
mod methods;
mod queries;

use std::any::Any;
use std::sync::{Arc, Mutex};

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};
use rbx_dom::editable::{self, lock, Editable, MeshSource, Triangles};
use rbx_dom::Ref;

use crate::datatypes::misc::LuaVector2;
use crate::datatypes::vector3::LuaVector3;
pub(crate) use data::MeshData;
pub(crate) use geometry::V3;
use ids::Id;

pub(crate) struct SharedMesh(pub(crate) Mutex<MeshData>);

impl MeshSource for SharedMesh {
    fn version(&self) -> u64 {
        lock(&self.0).version
    }

    fn triangles(&self) -> Triangles {
        let mesh = lock(&self.0);
        let computed = mesh.computed_normals(None);
        let mut out = Triangles::default();
        for (_, face) in mesh.faces.iter() {
            let positions = mesh.corners(face);
            let flat = mesh.face_normal(face);
            for triangle in data::corner_triangles(face, &positions) {
                for corner in triangle {
                    out.positions.push(positions[corner]);
                    let normal = match mesh.normals.get(face.normals[corner]) {
                        Some(Some(set)) => *set,
                        _ => computed.get(&face.normals[corner]).copied().unwrap_or(flat),
                    };
                    out.normals.push(normal);
                    out.uvs
                        .push(mesh.uvs.get(face.uvs[corner]).copied().unwrap_or_default());
                    out.colors.push(
                        mesh.colors
                            .get(face.colors[corner])
                            .copied()
                            .unwrap_or([1.0; 4]),
                    );
                }
            }
        }
        out
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

#[derive(Clone)]
pub(crate) struct LuaEditableMesh {
    pub(crate) referent: Ref,
    pub(crate) mesh: Arc<SharedMesh>,
    /// Alive while a script holds the handle it was made as (a client's
    /// budget counts it until then).
    pub(crate) token: Arc<()>,
}

impl LuaEditableMesh {
    pub(crate) fn read<T>(&self, f: impl FnOnce(&MeshData) -> Result<T>) -> Result<T> {
        f(&lock(&self.mesh.0))
    }

    pub(crate) fn edit<T>(&self, f: impl FnOnce(&mut MeshData) -> Result<T>) -> Result<T> {
        let mut mesh = lock(&self.mesh.0);
        let out = f(&mut mesh)?;
        mesh.version += 1;
        editable::touch();
        Ok(out)
    }
}

impl UserData for LuaEditableMesh {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Object");
        fields.add_field_method_get("FixedSize", |_, this| this.read(|m| Ok(m.fixed)));
        fields.add_field_method_get("ClassName", |_, _| Ok("EditableMesh"));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("EditableMesh"));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(referent(&other) == Some(this.referent))
        });
        methods.add_method("IsA", |_, _, class: String| {
            Ok(matches!(class.as_str(), "EditableMesh" | "Object"))
        });
        methods::add(methods);
        queries::add(methods);
        batch::add(methods);
        bones::add(methods);
    }
}

/// A number argument naming an id, with Roblox's message for anything
/// else; `position` counts from the first argument after `self`, which
/// Roblox's message counts too.
pub(crate) fn id_arg(value: &Value, position: usize) -> Result<f64> {
    match value {
        Value::Integer(i) => Ok(*i as f64),
        Value::Number(n) => Ok(*n),
        other => Err(mlua::Error::runtime(format!(
            "invalid argument #{} (int64 expected, got {})",
            position + 1,
            other.type_name()
        ))),
    }
}

pub(crate) fn read_vector(value: &Value) -> Result<V3> {
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

pub(crate) fn read_vector2(value: &Value) -> Result<[f32; 2]> {
    match value {
        Value::UserData(data) if data.is::<LuaVector2>() => {
            let v = data.borrow::<LuaVector2>()?.0;
            Ok([v.x, v.y])
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Vector2",
            other.type_name()
        ))),
    }
}

pub(crate) fn vector(v: V3) -> LuaVector3 {
    LuaVector3::new(v[0], v[1], v[2])
}

pub(crate) fn sequence(lua: &Lua, ids: impl IntoIterator<Item = Id>) -> Result<Table> {
    lua.create_sequence_from(ids.into_iter().map(|id| id as f64))
}

/// A table's numbers, for the methods taking a list of ids.
pub(crate) fn numbers(list: &Table) -> Result<Vec<f64>> {
    list.sequence_values::<Value>()
        .enumerate()
        .map(|(i, v)| id_arg(&v?, i + 1))
        .collect()
}

/// The referent an `EditableMesh` handle is published under.
pub(crate) fn referent(data: &AnyUserData) -> Option<Ref> {
    data.borrow::<LuaEditableMesh>()
        .ok()
        .map(|mesh| mesh.referent)
}

/// A handle on the published mesh `referent`.
pub(crate) fn handle(lua: &Lua, referent: Ref, mesh: Arc<dyn MeshSource>) -> Result<AnyUserData> {
    let mesh = mesh
        .into_any()
        .downcast::<SharedMesh>()
        .map_err(|_| mlua::Error::runtime("not an EditableMesh"))?;
    lua.create_userdata(LuaEditableMesh {
        referent,
        mesh,
        token: Arc::default(),
    })
}

pub(crate) fn new_mesh(lua: &Lua, data: MeshData) -> LuaEditableMesh {
    let mesh = Arc::new(SharedMesh(Mutex::new(data)));
    let referent = super::publish(lua, Editable::Mesh(mesh.clone()));
    LuaEditableMesh {
        referent,
        mesh,
        token: Arc::default(),
    }
}

/// A mesh of the triangles given, for an engine-made part (a fragment).
pub(crate) fn from_triangles(
    lua: &Lua,
    positions: &[V3],
    triangles: &[[u32; 3]],
) -> Result<LuaEditableMesh> {
    let mut data = MeshData::default();
    let ids: Vec<f64> = positions
        .iter()
        .map(|&p| data.add_vertex(p).map(|id| id as f64))
        .collect::<Result<_>>()?;
    for triangle in triangles {
        data.add_face(&triangle.map(|i| ids[i as usize]), "AddTriangle")?;
    }
    Ok(new_mesh(lua, data))
}
