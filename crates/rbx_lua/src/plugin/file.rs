//! `File`, what `StudioService:PromptImportFile[s]Async` gives: a file the
//! user picked, read when the plugin asks for its contents.

use std::path::PathBuf;

use mlua::{Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value};

pub(crate) struct LuaFile {
    path: PathBuf,
    id: u32,
}

impl LuaFile {
    pub(crate) fn make(lua: &Lua, path: PathBuf) -> Result<Value> {
        const KEY: &str = "rbxn.next_file_id";
        let id = lua.named_registry_value::<Option<u32>>(KEY)?.unwrap_or(0) + 1;
        lua.set_named_registry_value(KEY, id)?;
        Ok(Value::UserData(lua.create_userdata(LuaFile { path, id })?))
    }

    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl UserData for LuaFile {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let path = this.path.clone();
            let id = this.id;
            match key.as_str() {
                "Name" => Ok(Value::String(lua.create_string(this.name())?)),
                "ClassName" => Ok(Value::String(lua.create_string("File")?)),
                "Parent" => Ok(Value::Nil),
                "Size" => Ok(Value::Integer(
                    std::fs::metadata(&this.path).map_or(0, |m| m.len() as i64),
                )),
                "GetBinaryContents" => Ok(Value::Function(lua.create_function(
                    move |lua, _: mlua::MultiValue| {
                        let bytes = std::fs::read(&path).map_err(|e| {
                            mlua::Error::runtime(format!("Failed to read file: {e}"))
                        })?;
                        lua.create_string(bytes)
                    },
                )?)),
                "GetTemporaryId" => Ok(Value::Function(lua.create_function(
                    move |_, _: mlua::MultiValue| Ok(format!("rbxtemp://{id}")),
                )?)),
                "IsA" => Ok(Value::Function(lua.create_function(
                    |_, (_, class): (Value, String)| {
                        Ok(matches!(class.as_str(), "File" | "Instance"))
                    },
                )?)),
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of File \"{}\"",
                    this.name()
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.name()));
    }
}
