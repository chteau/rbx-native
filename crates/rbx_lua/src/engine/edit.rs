//! What edit mode answers for the services that only mean something in a
//! running game: `RunService`'s context checks (Roblox's RunService
//! reference lists Edit as studio, client and server at once, neither
//! running nor in run mode) and `Players`, which has nobody in it.

use mlua::{Lua, MultiValue, Result, Value};

pub(crate) fn member(lua: &Lua, class: &str, key: &str) -> Result<Option<Value>> {
    let flag = |on: bool| -> Result<Option<Value>> {
        let f = lua.create_function(move |_, _: MultiValue| Ok(on))?;
        Ok(Some(Value::Function(f)))
    };
    match (class, key) {
        ("RunService", "IsStudio" | "IsEdit" | "IsClient" | "IsServer") => flag(true),
        ("RunService", "IsRunning" | "IsRunMode") => flag(false),
        ("Players", "GetPlayers") => {
            let f = lua.create_function(|lua, _: MultiValue| lua.create_table())?;
            Ok(Some(Value::Function(f)))
        }
        ("Players", "LocalPlayer") => Ok(Some(Value::Nil)),
        _ => Ok(None),
    }
}
