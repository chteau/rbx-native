//! The `Batch*` methods: many elements per call. Adds and removes check
//! every entry before changing anything; `BatchSetValues` writes entries
//! until the first bad one (the EditableMeshBatch golden's `set_partial`).
//! Per-vertex face lists here come in id order, not newest first.

use mlua::{IntoLuaMulti, Lua, MultiValue, Result, Table, UserDataMethods, Value};

use super::data::{fixed_error, MeshData};
use super::geometry::V3;
use super::ids::{self, Id, Kind};
use super::methods::{alpha_byte, color3, set_face_attributes, set_face_vertices};
use super::queries::corner;
use super::{numbers, sequence, vector, LuaEditableMesh};
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::misc::LuaVector2;
use crate::datatypes::vector3::LuaVector3;
use crate::lua_enum::LuaEnumItem;

fn attribute(value: &Value) -> Result<Kind> {
    let name = match value {
        Value::UserData(data) if data.is::<LuaEnumItem>() => {
            data.borrow::<LuaEnumItem>()?.item_name().to_owned()
        }
        _ => String::new(),
    };
    Ok(match name.as_str() {
        "Vertex" => Kind::Vertex,
        "Normal" => Kind::Normal,
        "Color" => Kind::Color,
        "UV" => Kind::Uv,
        "Face" => Kind::Face,
        _ => {
            return Err(mlua::Error::runtime(format!(
                "Unable to cast {} to Enum.MeshAttribute",
                value.type_name()
            )))
        }
    })
}

/// What a value is, as Roblox's argument errors name it.
fn type_of(value: &Value) -> &'static str {
    match value {
        Value::UserData(d) if d.is::<LuaVector3>() => "Vector3",
        Value::UserData(d) if d.is::<LuaVector2>() => "Vector2",
        Value::UserData(d) if d.is::<LuaColor3>() => "Color3",
        Value::Number(_) | Value::Integer(_) => "number",
        other => other.type_name(),
    }
}

fn expect(value: &Value, wanted: &str, position: i32) -> Result<()> {
    match type_of(value) == wanted {
        true => Ok(()),
        false => Err(mlua::Error::runtime(format!(
            "invalid argument #{position} ({wanted} expected, got {})",
            type_of(value)
        ))),
    }
}

fn values(list: &Table) -> Result<Vec<Value>> {
    list.sequence_values::<Value>().collect()
}

fn v3(value: &Value) -> V3 {
    match value {
        Value::UserData(d) => d
            .borrow::<LuaVector3>()
            .map(|v| [v.0.x, v.0.y, v.0.z])
            .unwrap_or_default(),
        _ => [0.0; 3],
    }
}

fn v2(value: &Value) -> [f32; 2] {
    match value {
        Value::UserData(d) => d
            .borrow::<LuaVector2>()
            .map(|v| [v.0.x, v.0.y])
            .unwrap_or_default(),
        _ => [0.0; 2],
    }
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        _ => 0.0,
    }
}

fn batch_add(
    m: &mut MeshData,
    kind: Kind,
    data: &[Value],
    alphas: Option<&[Value]>,
) -> Result<Vec<Id>> {
    let wanted = match kind {
        Kind::Vertex | Kind::Normal => "Vector3",
        Kind::Uv => "Vector2",
        Kind::Color => "Color3",
        _ => "table",
    };
    for value in data {
        expect(value, wanted, -1)?;
    }
    if m.frozen() {
        return Err(fixed_error());
    }
    match kind {
        Kind::Vertex => {
            if m.vertices.len() + data.len() > super::data::MAX_VERTICES {
                return Err(mlua::Error::runtime("Vertex count above limit"));
            }
            data.iter().map(|v| m.add_vertex(v3(v))).collect()
        }
        Kind::Normal => Ok(data.iter().map(|v| m.normals.insert(Some(v3(v)))).collect()),
        Kind::Uv => Ok(data.iter().map(|v| m.uvs.insert(v2(v))).collect()),
        Kind::Color => {
            let alphas = alphas.unwrap_or_default();
            data.iter()
                .enumerate()
                .map(|(i, v)| {
                    let [r, g, b] = color3(v)?;
                    let a = alphas.get(i).map_or(1.0, number);
                    Ok(m.colors.insert([r, g, b, alpha_byte(a)]))
                })
                .collect()
        }
        _ => {
            let faces: Vec<Vec<Id>> = data
                .iter()
                .map(|face| {
                    let Value::Table(face) = face else { unreachable!() };
                    let list = numbers(face)?;
                    if !matches!(list.len(), 3 | 4) {
                        return Err(mlua::Error::runtime(format!(
                            "AddFace called with {} vertices, currently only supports 3 or 4 vertices",
                            list.len()
                        )));
                    }
                    list.iter().map(|&v| m.vertices.check(v)).collect()
                })
                .collect::<Result<_>>()?;
            let triangles: usize = faces.iter().map(|f| f.len() - 2).sum();
            if m.triangle_count() + triangles > super::data::MAX_TRIANGLES {
                return Err(mlua::Error::runtime("Triangle count above limit"));
            }
            let ids = m.reserve_faces(faces.len());
            for (&id, face) in ids.iter().zip(faces) {
                m.place_face(id, face);
            }
            Ok(ids)
        }
    }
}

/// `BatchGetValues`: the values for ids all of the first one's kind.
fn get_values(lua: &Lua, m: &MeshData, list: &[f64]) -> Result<MultiValue> {
    let Some(&first) = list.first() else {
        return (lua.create_table()?, lua.create_table()?).into_lua_multi(lua);
    };
    let kind = match ids::decode(first) {
        Some((kind @ (Kind::Vertex | Kind::Normal | Kind::Uv | Kind::Color), ..)) => kind,
        _ => return Err(ids::kind_error(Kind::Vertex, first)),
    };
    let out = lua.create_table()?;
    let alphas = lua.create_table()?;
    for (i, &id) in list.iter().enumerate() {
        let value = match kind {
            Kind::Vertex => {
                Value::UserData(lua.create_userdata(vector(m.position(m.vertices.check(id)?)))?)
            }
            Kind::Normal => {
                let n = m.normal(m.normals.check(id)?).unwrap_or_default();
                Value::UserData(lua.create_userdata(vector(n))?)
            }
            Kind::Uv => {
                let [x, y] = *m.uvs.get(m.uvs.check(id)?).expect("checked");
                Value::UserData(lua.create_userdata(LuaVector2::new(x, y))?)
            }
            _ => {
                let [r, g, b, a] = *m.colors.get(m.colors.check(id)?).expect("checked");
                alphas.raw_set(i + 1, a)?;
                Value::UserData(lua.create_userdata(LuaColor3::new(r, g, b))?)
            }
        };
        out.raw_set(i + 1, value)?;
    }
    match kind {
        Kind::Color => (out, alphas).into_lua_multi(lua),
        _ => out.into_lua_multi(lua),
    }
}

fn set_values(m: &mut MeshData, list: &[f64], data: Option<Vec<Value>>) -> Result<()> {
    if let Some(data) = &data {
        if data.len() != list.len() {
            return Err(mlua::Error::runtime(
                "BatchSetValues: ids and values arrays must be the same length",
            ));
        }
        if let Some(first) = data.first() {
            let wanted = type_of(first);
            for value in data {
                expect(value, wanted, 4)?;
            }
        }
    }
    for (i, &id) in list.iter().enumerate() {
        let kind = ids::decode(id).map(|(k, ..)| k);
        let value = data.as_ref().map(|d| &d[i]);
        match (kind, value) {
            (Some(Kind::Normal), None) => {
                let id = m.normals.check(id)?;
                *m.normals.get_mut(id).expect("checked") = None;
            }
            (Some(Kind::Normal), Some(v)) => {
                let id = m.normals.check(id)?;
                *m.normals.get_mut(id).expect("checked") = Some(v3(v));
            }
            (Some(Kind::Uv), Some(v)) => {
                let id = m.uvs.check(id)?;
                *m.uvs.get_mut(id).expect("checked") = v2(v);
            }
            (Some(Kind::Color), Some(v)) => {
                let id = m.colors.check(id)?;
                let c = m.colors.get_mut(id).expect("checked");
                match type_of(v) {
                    "number" => c[3] = alpha_byte(number(v)),
                    _ => {
                        let [r, g, b] = color3(v)?;
                        *c = [r, g, b, c[3]];
                    }
                }
            }
            (_, Some(v)) => {
                let id = m.vertices.check(id)?;
                m.vertices.get_mut(id).expect("checked").position = v3(v);
            }
            (_, None) => return Err(ids::kind_error(Kind::Normal, id)),
        }
    }
    Ok(())
}

/// The faces on `vertex` in id order.
fn faces_by_id(m: &MeshData, vertex: Id) -> Vec<Id> {
    let mut faces = m.vertex_faces.get(&vertex).cloned().unwrap_or_default();
    faces.sort_unstable();
    faces
}

pub(super) fn add<M: UserDataMethods<LuaEditableMesh>>(methods: &mut M) {
    methods.add_method(
        "BatchAdd",
        |lua, this, (attr, data, alphas): (Value, Table, Option<Table>)| {
            let kind = attribute(&attr)?;
            if kind == Kind::Color && alphas.is_none() {
                return Err(mlua::Error::runtime(
                    "BatchAdd(Enum.MeshAttribute.Color, ...): expects 2 arguments (colors, alphas)",
                ));
            }
            let data = values(&data)?;
            let alphas = alphas.map(|a| values(&a)).transpose()?;
            let added = this.edit(|m| batch_add(m, kind, &data, alphas.as_deref()))?;
            sequence(lua, added)
        },
    );
    methods.add_method("BatchGetValues", |lua, this, list: Table| {
        let list = numbers(&list)?;
        this.read(|m| get_values(lua, m, &list))
    });
    methods.add_method(
        "BatchSetValues",
        |_, this, (list, data): (Table, Option<Table>)| {
            let list = numbers(&list)?;
            let data = data.map(|d| values(&d)).transpose()?;
            this.edit(|m| set_values(m, &list, data))
        },
    );
    methods.add_method("BatchRemove", |_, this, list: Table| {
        let list = numbers(&list)?;
        this.edit(|m| {
            let faces: Vec<Id> = list
                .iter()
                .map(|&f| m.faces.check(f))
                .collect::<Result<_>>()?;
            if m.frozen() {
                return Err(fixed_error());
            }
            for f in faces {
                m.remove_face(f);
            }
            Ok(())
        })
    });
    methods.add_method(
        "BatchGetFaceAttributes",
        |lua, this, (attr, faces): (Value, Table)| {
            let kind = attribute(&attr)?;
            if kind == Kind::Face {
                return Err(mlua::Error::runtime(
                    "BatchGetFaceAttributes: Enum.MeshAttribute.Face is not supported",
                ));
            }
            let faces = numbers(&faces)?;
            this.read(|m| {
                let out = lua.create_table()?;
                for (i, &f) in faces.iter().enumerate() {
                    let f = m.faces.check(f)?;
                    out.raw_set(
                        i + 1,
                        sequence(lua, m.faces.get(f).expect("checked").slot(kind).clone())?,
                    )?;
                }
                Ok(out)
            })
        },
    );
    methods.add_method(
        "BatchGetVertexAttributes",
        |lua, this, (attr, vertices): (Value, Table)| {
            let kind = attribute(&attr)?;
            if kind == Kind::Vertex {
                return Err(mlua::Error::runtime(
                    "BatchGetVertexAttributes: Enum.MeshAttribute.Vertex is not supported",
                ));
            }
            let vertices = numbers(&vertices)?;
            this.read(|m| {
                let out = lua.create_table()?;
                for (i, &v) in vertices.iter().enumerate() {
                    let v = m.vertices.check(v)?;
                    let list: Vec<Id> = faces_by_id(m, v)
                        .into_iter()
                        .map(|f| match kind {
                            Kind::Face => f,
                            _ => {
                                let face = m.faces.get(f).expect("listed");
                                face.slot(kind)
                                    [face.vertices.iter().position(|&x| x == v).unwrap_or(0)]
                            }
                        })
                        .collect();
                    out.raw_set(i + 1, sequence(lua, list)?)?;
                }
                Ok(out)
            })
        },
    );
    methods.add_method(
        "BatchGetVertexFaceAttributes",
        |lua, this, (attr, vertices, faces): (Value, Table, Table)| {
            let kind = attribute(&attr)?;
            let (vertices, faces) = (numbers(&vertices)?, numbers(&faces)?);
            this.read(|m| {
                let list: Vec<Id> = vertices
                    .iter()
                    .zip(&faces)
                    .map(|(&v, &f)| {
                        let (v, f) = (m.vertices.check(v)?, m.faces.check(f)?);
                        let c = corner(m, v, f)?;
                        Ok(m.faces.get(f).expect("checked").slot(kind)[c])
                    })
                    .collect::<Result<_>>()?;
                sequence(lua, list)
            })
        },
    );
    methods.add_method("BatchSetFaceAttributes", |_, this, (faces, arrays): (Table, Table)| {
        let faces = numbers(&faces)?;
        let arrays: Vec<Vec<f64>> = arrays
            .sequence_values::<Table>()
            .map(|t| numbers(&t?))
            .collect::<Result<_>>()?;
        this.edit(|m| {
            for (i, (&f, list)) in faces.iter().zip(&arrays).enumerate() {
                let f = m.faces.check(f)?;
                let corners = m.faces.get(f).expect("checked").vertices.len();
                if list.len() != corners {
                    return Err(mlua::Error::runtime(format!(
                        "BatchSetFaceAttributes: inner array at index {} has {} elements but face has {corners} corners",
                        i + 1,
                        list.len()
                    )));
                }
                match list.first().and_then(|&id| ids::decode(id)).map(|(k, ..)| k) {
                    Some(Kind::Vertex) => set_face_vertices(m, f, list)?,
                    Some(kind @ (Kind::Normal | Kind::Uv | Kind::Color)) => set_face_attributes(m, f, kind, list)?,
                    _ => return Err(ids::kind_error(Kind::Normal, list.first().copied().unwrap_or(0.0))),
                }
            }
            Ok(())
        })
    });
    methods.add_method(
        "BatchSetVertexFaceAttributes",
        |_, this, (vertices, faces, attrs): (Table, Table, Table)| {
            let (vertices, faces, attrs) = (numbers(&vertices)?, numbers(&faces)?, numbers(&attrs)?);
            this.edit(|m| {
                for (i, ((&v, &f), &id)) in vertices.iter().zip(&faces).zip(&attrs).enumerate() {
                    let (v, f) = (m.vertices.check(v)?, m.faces.check(f)?);
                    let c = corner(m, v, f).map_err(|_| {
                        mlua::Error::runtime(format!(
                            "BatchSetVertexFaceAttributes: vertexId not found on requested face at index {}",
                            i + 1
                        ))
                    })?;
                    let kind = match ids::decode(id) {
                        Some((kind @ (Kind::Normal | Kind::Uv | Kind::Color), ..)) => kind,
                        _ => return Err(ids::kind_error(Kind::Normal, id)),
                    };
                    let id = match kind {
                        Kind::Normal => m.normals.check(id)?,
                        Kind::Uv => m.uvs.check(id)?,
                        _ => m.colors.check(id)?,
                    };
                    m.faces.get_mut(f).expect("checked").slot_mut(kind)[c] = id;
                }
                Ok(())
            })
        },
    );
}
