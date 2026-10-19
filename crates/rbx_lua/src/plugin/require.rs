//! `require` for plugin scripts, and the per-script globals Roblox gives
//! every script (`_G`, `shared`).
//!
//! Each script gets its own `require` (see `engine::require`). A string
//! argument is resolved against the requiring script the way
//! Luau's require-by-string does in Roblox: `./Name` is a sibling, `../`
//! climbs one more level, `@self/Name` is a child of the script itself and
//! `@game/Service/…` starts at a service.

use mlua::{Function, IntoLua, Lua, Result, Table, Value};
use rbx_dom::Ref;

use super::env::env;
use super::vm;
use crate::engine::require;
use crate::instance::LuaInstance;

const GLOBALS: &str = "rbxn.script_globals";

/// What `require` got: a `ModuleScript`, or a path from `script`.
fn resolve(lua: &Lua, script: Option<Ref>, target: &Value) -> Result<Ref> {
    match target {
        Value::Integer(_) | Value::Number(_) => {
            Err(mlua::Error::runtime("require by asset id is not supported"))
        }
        Value::String(path) => {
            let path = path.to_str()?.to_string();
            let script = script.ok_or_else(|| {
                mlua::Error::runtime(format!("cannot require '{path}' outside a script"))
            })?;
            let module = resolve_path(lua, script, &path)?;
            let instance = LuaInstance::new(module, env(lua).ctx.clone()).into_lua(lua)?;
            require::module_ref(lua, &instance)
        }
        other => require::module_ref(lua, other),
    }
}

fn resolve_path(lua: &Lua, script: Ref, path: &str) -> Result<Ref> {
    let env = env(lua);
    let dom = env.ctx.dom();
    let fail = || mlua::Error::runtime(format!("could not resolve require path '{path}'"));
    let mut parts = path.split('/').filter(|p| !p.is_empty()).peekable();
    let mut at = match parts.next().ok_or_else(fail)? {
        "." => dom.parent(script).ok_or_else(fail)?,
        ".." => {
            let parent = dom.parent(script).ok_or_else(fail)?;
            dom.parent(parent).ok_or_else(fail)?
        }
        "@self" => script,
        "@game" => {
            let service = parts.next().ok_or_else(fail)?;
            dom.root_refs()
                .iter()
                .copied()
                .find(|r| dom.get(*r).is_some_and(|i| i.name() == service))
                .ok_or_else(fail)?
        }
        _ => return Err(fail()),
    };
    for part in parts {
        at = match part {
            "." => at,
            ".." => dom.parent(at).ok_or_else(fail)?,
            name => crate::instance::tree::find_child(&dom, at, name, false).ok_or_else(fail)?,
        };
    }
    Ok(at)
}

/// Installs the plugin VM's `require` (see `engine::require`): paths are
/// resolved against the requiring script, a module compiles as a script
/// does (`vm::compile`).
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let resolve = lua.create_function(|lua, (script, target): (Value, Value)| {
        let script = match &script {
            Value::UserData(data) => data.borrow::<LuaInstance>().ok().map(|i| i.referent()),
            _ => None,
        };
        let module = resolve(lua, script, &target)?;
        LuaInstance::new(module, env(lua).ctx.clone()).into_lua(lua)
    })?;
    let compile = lua.create_function(|lua, module: Value| {
        let module = require::module_ref(lua, &module)?;
        vm::compile(lua, module)?.ok_or_else(|| mlua::Error::runtime(require::INVALID))
    })?;
    require::install(lua, resolve, compile)
}

/// A `require` bound to `script` (`nil` for one with no script).
pub(crate) fn require_for(lua: &Lua, script: Value) -> Result<Function> {
    require::require_for(lua, script)
}

/// A `getfenv` for one script: level 0 (the thread's environment) is the
/// script's own scope, as in Roblox where each script runs in a thread of
/// its own; any other level is the built-in one, one frame further out.
pub(crate) fn getfenv_for(lua: &Lua, scope: &Table) -> Result<Function> {
    const KEY: &str = "rbxn.getfenv_factory";
    let factory: Function = match lua.named_registry_value::<Option<Function>>(KEY)? {
        Some(found) => found,
        None => {
            let made: Function = lua
                .load(
                    r#"
                    local raw = getfenv
                    return function(scope)
                        return function(level)
                            if level == 0 then
                                return scope
                            elseif level == nil then
                                return raw(2)
                            elseif type(level) == "number" then
                                return raw(level + 1)
                            end
                            return raw(level)
                        end
                    end
                    "#,
                )
                .set_name("=getfenv")
                .eval()?;
            lua.set_named_registry_value(KEY, made.clone())?;
            made
        }
    };
    factory.call(scope.clone())
}

/// `_G` and `shared`: one writable table each per plugin VM, which every
/// script of the plugin sees (the real globals table is read-only once the
/// VM is sandboxed).
pub(crate) fn script_globals(lua: &Lua) -> Result<(Table, Table)> {
    if let Some(pair) = lua.named_registry_value::<Option<Table>>(GLOBALS)? {
        return Ok((pair.get(1)?, pair.get(2)?));
    }
    let (g, shared) = (lua.create_table()?, lua.create_table()?);
    lua.set_named_registry_value(
        GLOBALS,
        lua.create_sequence_from([g.clone(), shared.clone()])?,
    )?;
    Ok((g, shared))
}
