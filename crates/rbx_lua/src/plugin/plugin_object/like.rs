//! The `Instance` members a plugin may use on `plugin`, a toolbar or a
//! toolbar button, which in Roblox are instances (`Plugin`,
//! `PluginToolbar`, `PluginToolbarButton`) though here they are handles:
//! `ClassName`, `IsA`, a settable `Name`, attributes, `Parent` (nil),
//! `Destroy` (nothing to do) and the lowercase spellings.

use mlua::{Lua, Result, Table, Value};

const STORE: &str = "rbxn.handle_members";

/// This handle's own table of attributes and name, made on first use.
fn store(lua: &Lua, handle: &str) -> Result<Table> {
    let all = match lua.named_registry_value::<Option<Table>>(STORE)? {
        Some(all) => all,
        None => {
            let all = lua.create_table()?;
            lua.set_named_registry_value(STORE, all.clone())?;
            all
        }
    };
    if let Some(found) = all.get::<Option<Table>>(handle)? {
        return Ok(found);
    }
    let own = lua.create_table()?;
    own.set("attributes", lua.create_table()?)?;
    all.set(handle, own.clone())?;
    Ok(own)
}

/// `handle` names the object (`"toolbar:3"`), `class` its Roblox class.
pub(super) fn index(
    lua: &Lua,
    handle: &str,
    class: &'static str,
    key: &str,
) -> Result<Option<Value>> {
    let own = store(lua, handle)?;
    let attributes: Table = own.get("attributes")?;
    let value = match key {
        "ClassName" => Value::String(lua.create_string(class)?),
        "Name" => match own.get::<Value>("name")? {
            Value::Nil => return Ok(None),
            name => name,
        },
        "Parent" => Value::Nil,
        "IsA" => Value::Function(lua.create_function(move |_, (_, name): (Value, String)| {
            Ok(name == class || name == "Instance")
        })?),
        "Destroy" => Value::Function(lua.create_function(|_, _: Value| Ok(()))?),
        // Signals an instance has; a handle never fires them.
        "Destroying" | "Changed" | "AncestryChanged" | "ChildAdded" | "ChildRemoved"
        | "AttributeChanged" => {
            let key = crate::plugin::signal::SignalKey::new(
                crate::plugin::signal::Owner::Plugin,
                format!("{handle}.{key}"),
            );
            Value::UserData(lua.create_userdata(crate::plugin::signal::LuaSignal(key))?)
        }
        // No children: a handle is a leaf as far as scripts can tell.
        "FindFirstChild" | "FindFirstChildOfClass" | "FindFirstChildWhichIsA" | "WaitForChild" => {
            Value::Function(lua.create_function(|_, _: mlua::MultiValue| Ok(Value::Nil))?)
        }
        "GetChildren" | "GetDescendants" => {
            Value::Function(lua.create_function(|lua, _: Value| lua.create_table())?)
        }
        "GetAttribute" => {
            let attributes = attributes.clone();
            Value::Function(lua.create_function(move |_, (_, name): (Value, String)| {
                attributes.get::<Value>(name)
            })?)
        }
        "SetAttribute" => {
            let attributes = attributes.clone();
            Value::Function(lua.create_function(
                move |_, (_, name, value): (Value, String, Value)| attributes.set(name, value),
            )?)
        }
        "GetAttributes" => {
            let attributes = attributes.clone();
            Value::Function(lua.create_function(move |lua, _: Value| {
                let copy = lua.create_table()?;
                for pair in attributes.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    copy.set(k, v)?;
                }
                Ok(copy)
            })?)
        }
        _ => return crate::compat::lowercase_method(lua, key),
    };
    Ok(Some(value))
}

/// `Name = …` (kept) and `Parent = …` (ignored) on a handle; any other
/// assignment is not ours.
pub(super) fn new_index(lua: &Lua, handle: &str, key: &str, value: &Value) -> Result<bool> {
    match key {
        "Name" => store(lua, handle)?.set("name", value.clone())?,
        "Parent" => {}
        _ => return Ok(false),
    }
    Ok(true)
}

/// Keeps a settable field (`mouse.Icon`) for `field` to hand back.
pub(super) fn set_field(lua: &Lua, handle: &str, key: &str, value: Value) -> Result<()> {
    store(lua, handle)?.set(format!("field:{key}"), value)
}

/// A field `set_field` kept, if any.
pub(super) fn field(lua: &Lua, handle: &str, key: &str) -> Result<Value> {
    store(lua, handle)?.get(format!("field:{key}"))
}
