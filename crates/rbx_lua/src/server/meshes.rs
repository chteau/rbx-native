//! `AssetService`'s runtime meshes: `CreateEditableMesh` and
//! `CreateMeshPartAsync`, which turns an `EditableMesh` into a `MeshPart`
//! whose collision geometry the simulation is told about.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{
    Function, IntoLua, Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};

use super::Physics;
use crate::ctx::Ctx;
use crate::datatypes::content::{LuaContent, Source};
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;

#[derive(Default)]
struct MeshData {
    vertices: Vec<[f32; 3]>,
    faces: Vec<[usize; 3]>,
}

#[derive(Clone)]
struct LuaEditableMesh(Rc<RefCell<MeshData>>);

fn read_vector(value: &Value) -> Result<[f32; 3]> {
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

fn make_vector(lua: &Lua, v: [f32; 3]) -> Result<Value> {
    let new: Function = lua.globals().get::<Table>("Vector3")?.get("new")?;
    new.call((v[0], v[1], v[2]))
}

fn vertex_index(mesh: &MeshData, id: f64) -> Result<usize> {
    let index = id as usize;
    if id.fract() == 0.0 && index >= 1 && index <= mesh.vertices.len() {
        Ok(index - 1)
    } else {
        Err(mlua::Error::runtime(format!("Invalid vertex id {id}")))
    }
}

impl UserData for LuaEditableMesh {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "EditableMesh");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("AddVertex", |_, this, position: Value| {
            let mut mesh = this.0.borrow_mut();
            mesh.vertices.push(read_vector(&position)?);
            Ok(mesh.vertices.len())
        });
        methods.add_method("AddTriangle", |_, this, (a, b, c): (f64, f64, f64)| {
            let mut mesh = this.0.borrow_mut();
            let face = [
                vertex_index(&mesh, a)?,
                vertex_index(&mesh, b)?,
                vertex_index(&mesh, c)?,
            ];
            mesh.faces.push(face);
            Ok(mesh.faces.len())
        });
        methods.add_method("GetPosition", |lua, this, id: f64| {
            let mesh = this.0.borrow();
            make_vector(lua, mesh.vertices[vertex_index(&mesh, id)?])
        });
        methods.add_method("SetPosition", |_, this, (id, position): (f64, Value)| {
            let mut mesh = this.0.borrow_mut();
            let index = vertex_index(&mesh, id)?;
            mesh.vertices[index] = read_vector(&position)?;
            Ok(())
        });
        methods.add_method("GetVertices", |lua, this, ()| {
            let count = this.0.borrow().vertices.len();
            lua.create_sequence_from(1..=count)
        });
        methods.add_method("GetFaces", |lua, this, ()| {
            let count = this.0.borrow().faces.len();
            lua.create_sequence_from(1..=count)
        });
        methods.add_method("GetFaceVertices", |lua, this, id: f64| {
            let mesh = this.0.borrow();
            let index = id as usize;
            let face = (id.fract() == 0.0 && index >= 1)
                .then(|| mesh.faces.get(index - 1))
                .flatten()
                .ok_or_else(|| mlua::Error::runtime(format!("Invalid face id {id}")))?;
            lua.create_sequence_from(face.iter().map(|v| v + 1))
        });
        methods.add_method("Destroy", |_, this, ()| {
            *this.0.borrow_mut() = MeshData::default();
            Ok(())
        });
    }
}

fn editable_mesh(content: &Value) -> Result<Rc<RefCell<MeshData>>> {
    if let Value::UserData(data) = content {
        if let Ok(content) = data.borrow::<LuaContent>() {
            if let Source::Object(Value::UserData(object)) = &content.0 {
                if let Ok(mesh) = object.borrow::<LuaEditableMesh>() {
                    return Ok(mesh.0.clone());
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
    let mesh = editable_mesh(content)?;
    let (min, max, vertices, triangles) = {
        let mesh = mesh.borrow();
        if mesh.vertices.is_empty() {
            return Err(mlua::Error::runtime(
                "CreateMeshPartAsync: the mesh has no vertices",
            ));
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in &mesh.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(v[axis]);
                max[axis] = max[axis].max(v[axis]);
            }
        }
        // Roblox keeps the mesh's own origin as the part's origin (the
        // MeshCollision golden), so the vertices are not recentred.
        let vertices: Vec<[f32; 3]> = mesh.vertices.clone();
        let triangles: Vec<[u32; 3]> = mesh.faces.iter().map(|f| f.map(|i| i as u32)).collect();
        (min, max, vertices, triangles)
    };
    let referent = ctx.dom_mut().new_instance("MeshPart", "MeshPart", None);
    let part = LuaInstance::new(referent, ctx.clone()).into_lua(lua)?;
    let Value::UserData(instance) = &part else {
        return Err(mlua::Error::runtime("MeshPart could not be created"));
    };
    instance.set(
        "Size",
        make_vector(lua, std::array::from_fn(|i| max[i] - min[i]))?,
    )?;
    if let Some(options) = options {
        let fidelity: Value = options.get("CollisionFidelity")?;
        if !fidelity.is_nil() {
            instance.set("CollisionFidelity", fidelity)?;
        }
    }
    physics.register_mesh(referent, vertices, triangles);
    Ok(part)
}

pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    physics: &Rc<dyn Physics>,
    key: &str,
) -> Result<Option<Value>> {
    match key {
        "CreateEditableMesh" => {
            let f = lua.create_function(|_, _: mlua::MultiValue| {
                Ok(LuaEditableMesh(Rc::new(RefCell::new(MeshData::default()))))
            })?;
            Ok(Some(Value::Function(f)))
        }
        "CreateMeshPartAsync" => {
            let ctx = ctx.clone();
            let physics = physics.clone();
            let f = lua.create_function(
                move |lua, (_this, content, options): (Value, Value, Option<Table>)| {
                    create_mesh_part(lua, &ctx, &physics, &content, options)
                },
            )?;
            Ok(Some(Value::Function(f)))
        }
        _ => Ok(None),
    }
}
