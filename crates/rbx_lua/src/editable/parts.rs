//! `AssetService:CreateMeshPartAsync` and `MeshPart:ApplyMesh`, all local:
//! a part made from an asset id downloads the mesh, one made from an
//! editable binds it, and nothing is ever uploaded.
//!
//! What Roblox gives such a part (the EditableAssets golden): `Size` and
//! `MeshSize` the mesh's bounds, the mesh's own origin kept, `MeshId` the
//! asset's URI (empty for an editable), no parent. A later edit to the
//! mesh doesn't resize the part.

use mlua::{IntoLua, Lua, ObjectLike, Result, Table, Value};
use rbx_dom::{Content, Ref, Variant};

use super::assets::{download, error, snapshot_mesh};
use super::mesh::{vector, MeshData, V3};
use super::{as_mesh, target, Env, Target};
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;

const FIDELITIES: [&str; 3] = ["CollisionFidelity", "RenderFidelity", "FluidFidelity"];

fn options(options: &Option<Table>) -> Result<Vec<(&'static str, Value)>> {
    let mut out = Vec::new();
    let Some(options) = options else {
        return Ok(out);
    };
    for name in FIDELITIES {
        let value: Value = options.get(name)?;
        if value.is_nil() {
            continue;
        }
        let fits = match &value {
            Value::UserData(d) => d
                .borrow::<LuaEnumItem>()
                .is_ok_and(|item| item.enum_name() == name),
            _ => false,
        };
        if !fits {
            return Err(error(format!("'{name}' should be of type 'Enum.{name}'")));
        }
        out.push((name, value));
    }
    Ok(out)
}

/// Vertices in mesh order and triangles indexing them, for collision.
fn geometry(data: &MeshData) -> (Vec<V3>, Vec<[u32; 3]>) {
    let ids = data.vertices.ids();
    let index: std::collections::HashMap<u64, u32> =
        ids.iter().zip(0..).map(|(&id, i)| (id, i)).collect();
    let vertices = ids.iter().map(|&id| data.position(id)).collect();
    let mut triangles = Vec::new();
    for (_, face) in data.faces.iter() {
        let positions = data.corners(face);
        for t in super::mesh::data::corner_triangles(face, &positions) {
            triangles.push(t.map(|c| index[&face.vertices[c]]));
        }
    }
    (vertices, triangles)
}

fn size_of(data: &MeshData) -> V3 {
    let (min, max) = data.bounds();
    std::array::from_fn(|i| (max[i] - min[i]).max(0.001))
}

fn set(env: &Env, part: Ref, name: &str, value: Variant) -> Result<()> {
    env.ctx
        .dom_mut()
        .set_property(part, name, value)
        .map(drop)
        .map_err(|e| mlua::Error::runtime(e.to_string()))
}

/// Gives `part` the mesh: its size, content and collision.
fn give(env: &Env, part: Ref, data: &MeshData, mesh_id: &str, content: Content) -> Result<()> {
    let size = size_of(data);
    set(
        env,
        part,
        "MeshSize",
        Variant::Vector3(rbx_dom::Vector3Data {
            x: size[0],
            y: size[1],
            z: size[2],
        }),
    )?;
    set(env, part, "MeshId", Variant::String(mesh_id.to_owned()))?;
    set(env, part, "MeshContent", Variant::Content(content))?;
    if let Some(physics) = &env.physics {
        let (vertices, triangles) = geometry(data);
        physics.register_mesh(part, vertices, triangles);
    }
    Ok(())
}

pub(super) fn create_mesh_part(
    lua: &Lua,
    env: &Env,
    content: &Value,
    opts: Option<Table>,
) -> Result<Value> {
    let fidelities = options(&opts)?;
    let failed = || error("Failed to load mesh asset.");
    let (data, mesh_id, bound) = match target(content)? {
        Target::Uri(uri) => {
            let bytes = download(env, &uri, false).map_err(|_| failed())?;
            let data = super::mesh::load::from_file(&bytes).ok_or_else(failed)?;
            (data, uri.clone(), Content::Uri(uri))
        }
        Target::Object(object) => match as_mesh(&object) {
            Some(mesh) => (
                mesh.read(|m| Ok(m.clone()))?,
                String::new(),
                Content::Object(mesh.referent),
            ),
            None => {
                return Err(error(
                    "Invalid Content.Object. Expected EditableMesh, got Object",
                ))
            }
        },
        Target::Opaque(referent) => (
            snapshot_mesh(referent).ok_or_else(failed)?,
            String::new(),
            Content::Object(referent),
        ),
        Target::None => return Err(failed()),
    };
    if data.faces.len() == 0 {
        return Err(error(
            "Invalid mesh. Cannot create a MeshPart from an empty mesh. Mesh has no faces.",
        ));
    }
    let referent = env.ctx.dom_mut().new_instance("MeshPart", "MeshPart", None);
    let part = LuaInstance::new(referent, env.ctx.clone()).into_lua(lua)?;
    let Value::UserData(instance) = &part else {
        return Err(error("MeshPart could not be created"));
    };
    instance.set("Size", vector(size_of(&data)))?;
    let defaults: [Value; 3] = lua
        .load("return Enum.CollisionFidelity.Default, Enum.RenderFidelity.Automatic, Enum.FluidFidelity.Automatic")
        .eval::<(Value, Value, Value)>()
        .map(|(a, b, c)| [a, b, c])?;
    for (name, default) in FIDELITIES.into_iter().zip(defaults) {
        let given = fidelities
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone());
        instance.set(name, given.unwrap_or(default))?;
    }
    give(env, referent, &data, &mesh_id, bound)?;
    Ok(part)
}

/// `target:ApplyMesh(source)`: the source part's mesh, the target's size.
pub(super) fn apply_mesh(env: &Env, target_part: Ref, source: &Value) -> Result<()> {
    let wrong = || error("ApplyMesh requires an MeshPart object");
    let Value::UserData(data) = source else {
        return Err(wrong());
    };
    let source = data
        .borrow::<LuaInstance>()
        .map_err(|_| wrong())?
        .referent();
    let (size, mesh_id, content) = {
        let dom = env.ctx.dom();
        let instance = dom.get(source).ok_or_else(wrong)?;
        if instance.class() != "MeshPart" {
            return Err(wrong());
        }
        let read = |name: &str| instance.properties().get(name).cloned();
        (read("MeshSize"), read("MeshId"), read("MeshContent"))
    };
    for (name, value) in [
        ("MeshSize", size),
        ("MeshId", mesh_id),
        ("MeshContent", content.clone()),
    ] {
        if let Some(value) = value {
            set(env, target_part, name, value)?;
        }
    }
    if let (Some(physics), Some(Variant::Content(Content::Object(referent)))) =
        (&env.physics, content)
    {
        let data = match rbx_dom::editable::get(referent) {
            Some(rbx_dom::editable::Editable::Mesh(_)) => snapshot_mesh(referent),
            _ => None,
        };
        if let Some(data) = data {
            let (vertices, triangles) = geometry(&data);
            physics.register_mesh(target_part, vertices, triangles);
        }
    }
    Ok(())
}
