//! Services a place file does not carry but Roblox makes on first
//! `GetService` (`ContentProvider`, `ScriptEditorService`,
//! `KeyframeSequenceProvider`, `Geometry`, …). The plugin gets a parked
//! instance of the class — never part of the place — and the members it
//! is most likely to call.

use mlua::{IntoLua, Lua, MultiValue, Result, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::signal::{LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;
use crate::plugin::env::env;

/// The stand-in for service `name`, made on first use; `None` when `name`
/// is no service or the place already has one.
pub(super) fn service(lua: &Lua, name: &str) -> Result<Option<Value>> {
    let env = env(lua);
    let key = format!("rbxn.standin.{name}");
    if let Some(cached) = lua.named_registry_value::<Option<Value>>(&key)? {
        return Ok(Some(cached));
    }
    // Services that hold the place's own content are the file's to have
    // or not; only engine-side ones are stood in for.
    const PLACE_CONTENT: &[&str] = &[
        "Workspace",
        "Lighting",
        "Players",
        "ReplicatedFirst",
        "ReplicatedStorage",
        "ServerScriptService",
        "ServerStorage",
        "StarterGui",
        "StarterPack",
        "StarterPlayer",
        "Teams",
        "SoundService",
        "Chat",
        "TextChatService",
        "MaterialService",
    ];
    if !env.ctx.database().is_service(name) || PLACE_CONTENT.contains(&name) {
        return Ok(None);
    }
    let in_place = {
        let dom = env.ctx.dom();
        dom.root_refs()
            .iter()
            .any(|r| dom.get(*r).is_some_and(|i| i.class() == name))
    };
    if in_place {
        return Ok(None);
    }
    let referent = env.ctx.dom_mut().new_instance(name, name, None);
    env.roots.borrow_mut().push(referent);
    let value = LuaInstance::new(referent, env.ctx.clone()).into_lua(lua)?;
    lua.set_named_registry_value(&key, value.clone())?;
    Ok(Some(value))
}

/// Members of the stand-in classes; `None` for anything else.
pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match (class, key) {
        // Nothing to fetch ahead of time: assets load as they are drawn.
        ("ContentProvider", "PreloadAsync") => lua.create_function(|_, _: MultiValue| Ok(()))?,
        ("ContentProvider", "RequestQueueSize") => return Ok(Some(Value::Integer(0))),
        ("ScriptEditorService", "GetEditorSource") => {
            lua.create_function(move |_, (_, script): (Value, Value)| {
                let script = script_ref(&script)?;
                Ok(match ctx.dom().get(script).and_then(|i| i.properties().get("Source")) {
                    Some(Variant::String(source)) => source.clone(),
                    _ => String::new(),
                })
            })?
        }
        // Calls `transform(old)` and writes what it returns, as Studio does
        // (it would merge with an open editor; here the DOM is the copy).
        ("ScriptEditorService", "UpdateSourceAsync") => {
            lua.create_function(move |lua, (_, script, transform): (Value, Value, mlua::Function)| {
                let script = script_ref(&script)?;
                let old = match ctx.dom().get(script).and_then(|i| i.properties().get("Source")) {
                    Some(Variant::String(source)) => source.clone(),
                    _ => String::new(),
                };
                let new: Value = transform.call(old)?;
                let new = new.into_lua(lua)?;
                crate::property::set(&ctx, script, "Source", &new).map(|_| ())
            })?
        }
        ("ScriptEditorService", "GetScriptDocuments") => {
            lua.create_function(|lua, _: Value| lua.create_table())?
        }
        ("ScriptEditorService", "FindScriptDocument") => {
            lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?
        }
        ("ScriptEditorService", "TextDocumentDidOpen" | "TextDocumentDidClose" | "TextDocumentDidChange") => {
            let signal = LuaSignal(SignalKey::new(Owner::Instance(referent), key));
            return Ok(Some(Value::UserData(lua.create_userdata(signal)?)));
        }
        // Bound actions are kept for `GetBoundActionInfo`; the editor's
        // keys stay the editor's, so none is ever triggered.
        ("ContextActionService", "BindAction" | "BindActionAtPriority" | "UnbindAction"
            | "UnbindAllActions" | "SetTitle" | "SetImage" | "SetDescription" | "SetPosition") => {
            let bind = matches!(key, "BindAction" | "BindActionAtPriority");
            let unbind = key == "UnbindAction";
            lua.create_function(move |lua, (_, name, rest): (Value, Value, MultiValue)| {
                let bound = bound_actions(lua)?;
                if bind {
                    let info = lua.create_table()?;
                    info.set("callback", rest.front().cloned().unwrap_or(Value::Nil))?;
                    bound.set(name, info)?;
                } else if unbind {
                    bound.set(name, Value::Nil)?;
                }
                Ok(())
            })?
        }
        ("ContextActionService", "GetBoundActionInfo") => {
            lua.create_function(|lua, (_, name): (Value, Value)| {
                match bound_actions(lua)?.get::<Value>(name)? {
                    Value::Nil => Ok(Value::Table(lua.create_table()?)),
                    info => Ok(info),
                }
            })?
        }
        ("ContextActionService", "GetAllBoundActionInfo") => {
            lua.create_function(|lua, _: Value| bound_actions(lua))?
        }
        // Studio's edit view has no top bar.
        ("GuiService", "GetGuiInset") => lua.create_function(|lua, _: MultiValue| {
            let zero: Value = lua.load("return Vector2.zero").eval()?;
            Ok((zero.clone(), zero))
        })?,
        ("GuiService", "IsTenFootInterface") => lua.create_function(|_, _: MultiValue| Ok(false))?,
        ("ContextActionService", "GetButton") => {
            lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?
        }
        // A sequence registered for preview gets a temporary id, as in
        // Studio; reading one back needs Roblox's servers.
        ("KeyframeSequenceProvider", "RegisterKeyframeSequence") => {
            lua.create_function(|lua, _: MultiValue| {
                let next: i64 = lua.named_registry_value::<Option<i64>>("rbxn.temp_ids")?.unwrap_or(0) + 1;
                lua.set_named_registry_value("rbxn.temp_ids", next)?;
                Ok(format!("rbxtemp://{next}"))
            })?
        }
        ("KeyframeSequenceProvider", "GetKeyframeSequenceAsync") => {
            lua.create_function(|_, _: MultiValue| -> Result<()> {
                Err(mlua::Error::runtime(
                    "GetKeyframeSequenceAsync needs Roblox's servers, which rbx-native does not reach",
                ))
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

fn bound_actions(lua: &Lua) -> Result<mlua::Table> {
    const KEY: &str = "rbxn.bound_actions";
    if let Some(found) = lua.named_registry_value::<Option<mlua::Table>>(KEY)? {
        return Ok(found);
    }
    let made = lua.create_table()?;
    lua.set_named_registry_value(KEY, made.clone())?;
    Ok(made)
}

fn script_ref(value: &Value) -> Result<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
        .ok_or_else(|| mlua::Error::runtime("a script expected"))
}
