//! `EditableImage` and `EditableMesh`, and the `AssetService` calls that
//! make them, for every VM that has them: a Local Run's server and
//! clients, the command bar and plugins in Edit. Each editable is
//! published in `rbx_dom::editable`, where a renderer finds it through the
//! `Content::Object` a property holds.

mod assets;
pub(crate) mod budget;
pub(crate) mod image;
pub(crate) mod mesh;
mod parts;

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use mlua::{AnyUserData, IntoLuaMulti, Lua, Result, Table, Value};
use rbx_dom::editable::{self, lock, Editable};
use rbx_dom::Ref;

use crate::ctx::Ctx;
use crate::datatypes::content::{LuaContent, Source};
use crate::server::{AssetSource, Physics};
use image::LuaEditableImage;
use mesh::LuaEditableMesh;

/// Every editable a VM published, withdrawn when the VM goes: a run that
/// ends must not leave its canvases behind for the next one.
#[derive(Default)]
struct Owned(Vec<Ref>);

impl Drop for Owned {
    fn drop(&mut self) {
        for referent in &self.0 {
            editable::withdraw(*referent);
            lock(&OPAQUE).remove(referent);
        }
    }
}

/// Publishes `editable` and ties its life to `lua`'s.
pub(crate) fn publish(lua: &Lua, editable: Editable) -> Ref {
    let referent = editable::publish(editable);
    match lua.app_data_mut::<Owned>() {
        Some(mut owned) => owned.0.push(referent),
        None => {
            lua.set_app_data(Owned(vec![referent]));
        }
    }
    referent
}

/// The snapshots `CreateDataModelContentAsync` made, which read back as
/// opaque content rather than an object.
static OPAQUE: LazyLock<Mutex<HashSet<Ref>>> = LazyLock::new(Mutex::default);

/// Where editables made from asset ids download from when the VM has no
/// source of its own (the command bar, plugins): the editor's.
static FALLBACK_ASSETS: OnceLock<Arc<dyn AssetSource>> = OnceLock::new();

/// Sets where Edit's editables download assets from.
pub fn set_asset_source(source: Arc<dyn AssetSource>) {
    let _ = FALLBACK_ASSETS.set(source);
}

/// The referent an `EditableImage`/`EditableMesh` handle is published under,
/// what a `Content` property stores.
pub(crate) fn editable_ref(data: &AnyUserData) -> Option<Ref> {
    if let Ok(image) = data.borrow::<LuaEditableImage>() {
        return Some(image.referent);
    }
    mesh::referent(data)
}

/// A handle on the editable `referent`, if it is still there.
fn handle(lua: &Lua, referent: Ref) -> Result<Option<AnyUserData>> {
    Ok(Some(match editable::get(referent) {
        Some(Editable::Image(pixels)) => lua.create_userdata(LuaEditableImage {
            referent,
            pixels,
            token: Arc::default(),
        })?,
        Some(Editable::Mesh(mesh)) => mesh::handle(lua, referent, mesh)?,
        None => return Ok(None),
    }))
}

/// A `Content` for a `Content` property holding the editable `referent`;
/// `None` once it is gone.
pub(crate) fn editable_content(lua: &Lua, referent: Ref) -> Result<Option<Value>> {
    let source = match lock(&OPAQUE).contains(&referent) {
        true => Source::Opaque(referent),
        false => match handle(lua, referent)? {
            Some(object) => Source::Object(Value::UserData(object)),
            None => return Ok(None),
        },
    };
    lua.create_userdata(LuaContent(source))
        .map(|c| Some(Value::UserData(c)))
}

/// What the VM calling `AssetService` can reach.
pub(crate) struct Env {
    pub(crate) ctx: Ctx,
    /// The simulation, told about runtime `MeshPart`s; none in Edit.
    pub(crate) physics: Option<Rc<dyn Physics>>,
    pub(crate) assets: Option<Arc<dyn AssetSource>>,
    pub(crate) owners: Vec<u64>,
    pub(crate) cache_dir: Option<PathBuf>,
    /// A client, whose editables come out of a memory budget.
    pub(crate) client: bool,
    /// The command bar or a plugin rather than a running game.
    pub(crate) edit: bool,
}

impl Env {
    /// Edit's: the command bar's or a plugin's.
    pub(crate) fn edit(ctx: &Ctx) -> Env {
        Env {
            ctx: ctx.clone(),
            physics: None,
            assets: None,
            owners: Vec::new(),
            cache_dir: None,
            client: false,
            edit: true,
        }
    }

    fn assets(&self) -> Option<Arc<dyn AssetSource>> {
        self.assets
            .clone()
            .or_else(|| FALLBACK_ASSETS.get().cloned())
    }
}

/// What a `Content` argument names.
pub(crate) enum Target {
    None,
    Uri(String),
    Object(AnyUserData),
    Opaque(Ref),
}

pub(crate) fn target(value: &Value) -> Result<Target> {
    Ok(match value {
        Value::String(uri) => Target::Uri(uri.to_str()?.to_owned()),
        Value::Nil => Target::None,
        Value::UserData(data) if data.is::<LuaContent>() => match &data.borrow::<LuaContent>()?.0 {
            Source::None => Target::None,
            Source::Uri(uri) => Target::Uri(uri.clone()),
            Source::Object(Value::UserData(object)) => Target::Object(object.clone()),
            Source::Object(_) => Target::None,
            Source::Opaque(referent) => Target::Opaque(*referent),
        },
        other => {
            return Err(mlua::Error::runtime(format!(
                "Unable to cast {} to Content",
                other.type_name()
            )))
        }
    })
}

/// A snapshot published for opaque content, or one read back from it.
fn opaque_copy(referent: Ref) -> Option<Editable> {
    Some(match editable::get(referent)? {
        Editable::Image(pixels) => {
            let mut copy = lock(&pixels).clone();
            copy.version = 0;
            Editable::Image(Arc::new(Mutex::new(copy)))
        }
        Editable::Mesh(source) => {
            let shared = source.into_any().downcast::<mesh::SharedMesh>().ok()?;
            let data = lock(&shared.0).clone();
            Editable::Mesh(Arc::new(mesh::SharedMesh(Mutex::new(data))))
        }
    })
}

fn data_model_content(lua: &Lua, content: &Value) -> Result<mlua::MultiValue> {
    let referent = match target(content)? {
        Target::Object(object) => editable_ref(&object),
        Target::Opaque(referent) => Some(referent),
        _ => None,
    };
    let snapshot = referent.and_then(opaque_copy).ok_or_else(|| {
        mlua::Error::runtime(
            "The input content type is not supported by AssetService::CreateDataModelContentAsync yet!",
        )
    })?;
    let referent = publish(lua, snapshot);
    lock(&OPAQUE).insert(referent);
    let success: Value = lua.load("return Enum.CreateContentResult.Success").eval()?;
    (success, LuaContent(Source::Opaque(referent))).into_lua_multi(lua)
}

fn fixed_option(options: &Option<Table>) -> Result<bool> {
    match options {
        Some(options) => Ok(options.get::<Option<bool>>("FixedSize")?.unwrap_or(true)),
        None => Ok(true),
    }
}

/// A new mesh, or nothing when a client's budget has no room for it.
fn budgeted(lua: &Lua, env: &Env, data: mesh::MeshData) -> Result<Option<LuaEditableMesh>> {
    let cost = budget::mesh_cost(&data);
    if !budget::admit(lua, env.client, true, cost) {
        return Ok(None);
    }
    let made = mesh::new_mesh(lua, data);
    budget::hold(lua, env.client, made.referent, true, cost, &made.token);
    Ok(Some(made))
}

/// `AssetService`'s editable members; `None` for any other key.
pub(crate) fn member(lua: &Lua, env: &Rc<Env>, key: &str) -> Result<Option<Value>> {
    let env = env.clone();
    let f = match key {
        "CreateEditableImage" => {
            lua.create_function(move |lua, (_this, options): (Value, Option<Table>)| {
                image::create(lua, &env, options)
            })?
        }
        "CreateEditableImageAsync" => lua.create_function(
            move |lua, (_this, content, _options): (Value, Value, Option<Table>)| {
                assets::image_async(lua, &env, &content)
            },
        )?,
        // An empty mesh is never fixed, whatever the options ask.
        "CreateEditableMesh" => lua.create_function(move |lua, _: mlua::MultiValue| {
            budgeted(lua, &env, mesh::MeshData::default())
        })?,
        "CreateEditableMeshAsync" => lua.create_function(
            move |lua, (_this, content, options): (Value, Value, Option<Table>)| {
                let fixed = fixed_option(&options)?;
                let mut data = assets::mesh_data(&env, &content)?;
                data.fixed = fixed;
                data.in_edit = env.edit;
                data.version = 0;
                budgeted(lua, &env, data)
            },
        )?,
        "CreateMeshPartAsync" => lua.create_function(
            move |lua, (_this, content, options): (Value, Value, Option<Table>)| {
                parts::create_mesh_part(lua, &env, &content, options)
            },
        )?,
        "CreateDataModelContentAsync" => {
            lua.create_function(|lua, (_this, content): (Value, Value)| {
                data_model_content(lua, &content)
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// `MeshPart:ApplyMesh`; `None` for any other key.
pub(crate) fn mesh_part_member(
    lua: &Lua,
    env: &Rc<Env>,
    referent: Ref,
    key: &str,
) -> Result<Option<Value>> {
    if key != "ApplyMesh" {
        return Ok(None);
    }
    let env = env.clone();
    let f = lua.create_function(move |_, (_this, source): (Value, Value)| {
        parts::apply_mesh(&env, referent, &source)
    })?;
    Ok(Some(Value::Function(f)))
}

/// The engine's own mesh on `part` (a fragment, say): the triangles given,
/// through an `EditableMesh` its `MeshContent` holds.
pub(crate) fn bind(
    lua: &Lua,
    ctx: &Ctx,
    part: Ref,
    positions: &[mesh::V3],
    triangles: &[[u32; 3]],
) -> Result<()> {
    let made = mesh::from_triangles(lua, positions, triangles)?;
    ctx.dom_mut()
        .set_property(
            part,
            "MeshContent",
            rbx_dom::Variant::Content(rbx_dom::Content::Object(made.referent)),
        )
        .map(drop)
        .map_err(|error| mlua::Error::runtime(error.to_string()))
}

/// The mesh or image a handle is, for the methods taking either.
pub(crate) fn as_mesh(object: &AnyUserData) -> Option<LuaEditableMesh> {
    object.borrow::<LuaEditableMesh>().ok().map(|m| m.clone())
}
