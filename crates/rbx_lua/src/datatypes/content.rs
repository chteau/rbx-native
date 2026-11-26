//! `Content`: a reference to an asset by URI or to an in-memory object
//! (an `EditableMesh`), as `AssetService:CreateMeshPartAsync` takes it.

use mlua::{
    Function, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};

#[derive(Clone)]
pub(crate) enum Source {
    None,
    Uri(String),
    Object(Value),
    /// `AssetService:CreateDataModelContentAsync`'s snapshot: an editable
    /// published under this referent, which scripts can't reach.
    Opaque(rbx_dom::Ref),
}

#[derive(Clone)]
pub(crate) struct LuaContent(pub(crate) Source);

fn source_type(lua: &Lua, name: &str) -> Result<Value> {
    let lookup: Function = lua
        .load("local name = ... return Enum.ContentSourceType[name]")
        .into_function()?;
    lookup.call(name)
}

impl UserData for LuaContent {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Content");
        fields.add_field_method_get("SourceType", |lua, this| {
            source_type(
                lua,
                match this.0 {
                    Source::None => "None",
                    Source::Uri(_) => "Uri",
                    Source::Object(_) => "Object",
                    Source::Opaque(_) => "Opaque",
                },
            )
        });
        fields.add_field_method_get("Uri", |lua, this| {
            Ok(match &this.0 {
                Source::Uri(uri) => Value::String(lua.create_string(uri)?),
                _ => Value::Nil,
            })
        });
        fields.add_field_method_get("Object", |_, this| {
            Ok(match &this.0 {
                Source::Object(object) => object.clone(),
                _ => Value::Nil,
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(_methods: &mut M) {}
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("none", LuaContent(Source::None))?;
    table.set(
        "fromUri",
        lua.create_function(|_, uri: String| Ok(LuaContent(Source::Uri(uri))))?,
    )?;
    table.set(
        "fromAssetId",
        lua.create_function(|_, id: i64| {
            Ok(LuaContent(Source::Uri(format!("rbxassetid://{id}"))))
        })?,
    )?;
    table.set(
        "fromObject",
        lua.create_function(|_, object: Value| match object {
            Value::UserData(_) => Ok(LuaContent(Source::Object(object))),
            other => Err(mlua::Error::runtime(format!(
                "Content.fromObject: Object expected, got {}",
                other.type_name()
            ))),
        })?,
    )?;
    Ok(table)
}
