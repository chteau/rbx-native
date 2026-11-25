//! Building and editing: elements added and removed, positions and
//! attribute values, each face's corners.

use mlua::{Lua, MultiValue, Result, Table, UserDataMethods, Value};

use super::data::{fixed_error, MeshData};
use super::ids::{self, Id, Kind};
use super::{id_arg, numbers, read_vector, read_vector2, sequence, vector, LuaEditableMesh};
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::misc::LuaVector2;

/// Roblox keeps a colour's alpha as a byte, `floor(alpha * 256)` (0.3
/// reads back 0.298, 0.25 reads 0.251).
pub(super) fn alpha_byte(alpha: f64) -> f32 {
    ((alpha * 256.0).floor().clamp(0.0, 255.0) / 255.0) as f32
}

fn adding(m: &MeshData) -> Result<()> {
    match m.frozen() {
        true => Err(fixed_error()),
        false => Ok(()),
    }
}

pub(super) fn color3(value: &Value) -> Result<[f32; 3]> {
    match value {
        Value::UserData(data) if data.is::<LuaColor3>() => {
            let c = data.borrow::<LuaColor3>()?;
            Ok([c.0.r, c.0.g, c.0.b])
        }
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to Color3",
            other.type_name()
        ))),
    }
}

fn kind_of(which: &str) -> Kind {
    match which {
        "Normals" => Kind::Normal,
        "UVs" => Kind::Uv,
        "Colors" => Kind::Color,
        _ => Kind::Vertex,
    }
}

/// `SetFace{Normals,UVs,Colors}`: one id of the kind per corner.
pub(super) fn set_face_attributes(
    m: &mut MeshData,
    face: Id,
    kind: Kind,
    list: &[f64],
) -> Result<()> {
    let corners = m.faces.get(face).expect("checked").vertices.len();
    if list.len() != corners {
        return Err(mlua::Error::runtime(
            "Number of attributes provided does not match face",
        ));
    }
    let checked: Vec<Id> = list
        .iter()
        .map(|&id| match kind {
            Kind::Normal => m.normals.check(id),
            Kind::Uv => m.uvs.check(id),
            _ => m.colors.check(id),
        })
        .collect::<Result<_>>()?;
    *m.faces.get_mut(face).expect("checked").slot_mut(kind) = checked;
    Ok(())
}

pub(super) fn set_face_vertices(m: &mut MeshData, face: Id, list: &[f64]) -> Result<()> {
    if !matches!(list.len(), 3 | 4) {
        return Err(mlua::Error::runtime(
            "SetFaceVertices: only 3 or 4 vertices supported",
        ));
    }
    let vertices: Vec<Id> = list
        .iter()
        .map(|&v| m.vertices.check(v))
        .collect::<Result<_>>()?;
    m.set_face_vertices(face, vertices)
}

fn face_list(lua: &Lua, this: &LuaEditableMesh, face: f64, kind: Kind) -> Result<Table> {
    this.read(|m| {
        let face = m.faces.check(face)?;
        sequence(
            lua,
            m.faces
                .get(face)
                .expect("checked")
                .slot(kind)
                .iter()
                .copied(),
        )
    })
}

pub(super) fn add<M: UserDataMethods<LuaEditableMesh>>(methods: &mut M) {
    methods.add_method("IdDebugString", |_, _, id: Value| {
        Ok(ids::debug_string(id_arg(&id, 1)?))
    });
    methods.add_method("AddVertex", |_, this, p: Value| {
        let p = read_vector(&p)?;
        this.edit(|m| {
            adding(m)?;
            m.add_vertex(p).map(|id| id as f64)
        })
    });
    methods.add_method("AddFace", |_, this, args: MultiValue| {
        let list: Vec<f64> = args
            .iter()
            .enumerate()
            .map(|(i, v)| id_arg(v, i + 1))
            .collect::<Result<_>>()?;
        this.edit(|m| m.add_face(&list, "AddFace").map(|id| id as f64))
    });
    methods.add_method(
        "AddTriangle",
        |_, this, (a, b, c): (Value, Value, Value)| {
            let list = [id_arg(&a, 1)?, id_arg(&b, 2)?, id_arg(&c, 3)?];
            this.edit(|m| m.add_face(&list, "AddTriangle").map(|id| id as f64))
        },
    );
    methods.add_method("AddNormal", |_, this, normal: Option<Value>| {
        let normal = normal
            .filter(|v| !v.is_nil())
            .map(|v| read_vector(&v))
            .transpose()?;
        this.edit(|m| {
            adding(m)?;
            Ok(m.normals.insert(normal) as f64)
        })
    });
    methods.add_method("AddUV", |_, this, uv: Value| {
        let uv = read_vector2(&uv)?;
        this.edit(|m| {
            adding(m)?;
            Ok(m.uvs.insert(uv) as f64)
        })
    });
    methods.add_method("AddColor", |_, this, (color, alpha): (Value, f64)| {
        let [r, g, b] = color3(&color)?;
        this.edit(|m| {
            adding(m)?;
            Ok(m.colors.insert([r, g, b, alpha_byte(alpha)]) as f64)
        })
    });
    methods.add_method("RemoveFace", |_, this, face: Value| {
        let face = id_arg(&face, 1)?;
        this.edit(|m| {
            match ids::decode(face) {
                Some((Kind::Face, ..)) => {}
                _ => return Err(ids::kind_error(Kind::Face, face)),
            }
            if !m.faces.contains(face as Id) {
                return Err(mlua::Error::runtime("Invalid faceId"));
            }
            adding(m)?;
            m.remove_face(face as Id);
            Ok(())
        })
    });
    methods.add_method("RemoveUnused", |lua, this, ()| {
        let gone = this.edit(|m| {
            adding(m)?;
            Ok(m.remove_unused())
        })?;
        sequence(lua, gone)
    });
    methods.add_method("Clear", |_, this, ()| {
        this.edit(|m| {
            adding(m)?;
            *m = MeshData {
                version: m.version,
                ..MeshData::default()
            };
            Ok(())
        })
    });
    methods.add_method("Triangulate", |_, this, ()| this.edit(|m| m.triangulate()));
    methods.add_method("MergeVertices", |lua, this, tolerance: f32| {
        this.edit(|m| {
            m.merge_vertices(tolerance);
            Ok(())
        })?;
        lua.create_table()
    });

    methods.add_method("GetPosition", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| Ok(vector(m.position(m.vertices.check(id)?))))
    });
    methods.add_method("SetPosition", |_, this, (id, p): (Value, Value)| {
        let (id, p) = (id_arg(&id, 1)?, read_vector(&p)?);
        this.edit(|m| {
            let id = m.vertices.check(id)?;
            m.vertices.get_mut(id).expect("checked").position = p;
            Ok(())
        })
    });
    methods.add_method("GetVertices", |lua, this, ()| {
        this.read(|m| sequence(lua, m.vertices.ids()))
    });
    methods.add_method("GetFaces", |lua, this, ()| {
        this.read(|m| sequence(lua, m.faces.ids()))
    });
    methods.add_method("GetNormals", |lua, this, ()| {
        this.read(|m| sequence(lua, m.normals.ids()))
    });
    methods.add_method("GetUVs", |lua, this, ()| {
        this.read(|m| sequence(lua, m.uvs.ids()))
    });
    methods.add_method("GetColors", |lua, this, ()| {
        this.read(|m| sequence(lua, m.colors.ids()))
    });
    for which in ["Vertices", "Normals", "UVs", "Colors"] {
        let kind = kind_of(which);
        methods.add_method(format!("GetFace{which}"), move |lua, this, face: Value| {
            face_list(lua, this, id_arg(&face, 1)?, kind)
        });
        methods.add_method(
            format!("SetFace{which}"),
            move |_, this, (face, list): (Value, Table)| {
                let (face, list) = (id_arg(&face, 1)?, numbers(&list)?);
                this.edit(|m| {
                    let face = m.faces.check(face)?;
                    match kind {
                        Kind::Vertex => set_face_vertices(m, face, &list),
                        _ => set_face_attributes(m, face, kind, &list),
                    }
                })
            },
        );
    }

    methods.add_method("GetNormal", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| Ok(vector(m.normal(m.normals.check(id)?).unwrap_or_default())))
    });
    methods.add_method("SetNormal", |_, this, (id, normal): (Value, Value)| {
        let (id, normal) = (id_arg(&id, 1)?, read_vector(&normal)?);
        this.edit(|m| {
            let id = m.normals.check(id)?;
            *m.normals.get_mut(id).expect("checked") = Some(normal);
            Ok(())
        })
    });
    methods.add_method("ResetNormal", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.edit(|m| {
            let id = m.normals.check(id)?;
            *m.normals.get_mut(id).expect("checked") = None;
            Ok(())
        })
    });
    methods.add_method("GetUV", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| {
            let [x, y] = *m.uvs.get(m.uvs.check(id)?).expect("checked");
            Ok(LuaVector2::new(x, y))
        })
    });
    methods.add_method("SetUV", |_, this, (id, uv): (Value, Value)| {
        let (id, uv) = (id_arg(&id, 1)?, read_vector2(&uv)?);
        this.edit(|m| {
            let id = m.uvs.check(id)?;
            *m.uvs.get_mut(id).expect("checked") = uv;
            Ok(())
        })
    });
    methods.add_method("GetColor", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| {
            let [r, g, b, _] = *m.colors.get(m.colors.check(id)?).expect("checked");
            Ok(LuaColor3::new(r, g, b))
        })
    });
    methods.add_method("SetColor", |_, this, (id, color): (Value, Value)| {
        let (id, [r, g, b]) = (id_arg(&id, 1)?, color3(&color)?);
        this.edit(|m| {
            let id = m.colors.check(id)?;
            let c = m.colors.get_mut(id).expect("checked");
            *c = [r, g, b, c[3]];
            Ok(())
        })
    });
    methods.add_method("GetColorAlpha", |_, this, id: Value| {
        let id = id_arg(&id, 1)?;
        this.read(|m| Ok(m.colors.get(m.colors.check(id)?).expect("checked")[3]))
    });
    methods.add_method("SetColorAlpha", |_, this, (id, alpha): (Value, f64)| {
        let id = id_arg(&id, 1)?;
        this.edit(|m| {
            let id = m.colors.check(id)?;
            m.colors.get_mut(id).expect("checked")[3] = alpha_byte(alpha);
            Ok(())
        })
    });

    methods.add_method("GetSize", |_, this, ()| {
        this.read(|m| {
            let (min, max) = m.bounds();
            Ok(vector(std::array::from_fn(|i| max[i] - min[i])))
        })
    });
    methods.add_method("GetCenter", |_, this, ()| {
        this.read(|m| {
            let (min, max) = m.bounds();
            Ok(vector(std::array::from_fn(|i| (min[i] + max[i]) / 2.0)))
        })
    });
    methods.add_method("Destroy", |lua, this, ()| {
        rbx_dom::editable::withdraw(this.referent);
        super::super::budget::release(lua, this.referent);
        let mut mesh = rbx_dom::editable::lock(&this.mesh.0);
        let version = mesh.version + 1;
        *mesh = MeshData {
            version,
            ..MeshData::default()
        };
        Ok(())
    });
}
