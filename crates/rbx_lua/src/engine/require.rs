//! `require` of a `ModuleScript`, built in Lua around a few Rust pieces so
//! a module body runs in a Lua frame: it may yield (`task.wait`, an HTTP
//! call) as in Roblox, which a module called from Rust could not.
//!
//! Each VM says how a `require` argument names a module and how a module's
//! `Source` becomes a function (see [`install`]); caching and the
//! recursion check are shared.

use std::collections::{HashMap, HashSet};

use mlua::{Function, Lua, Result, Value};
use rbx_dom::Ref;

use super::engine;
use crate::instance::LuaInstance;

const FACTORY: &str = "rbxn.require_factory";

/// Roblox's error for anything but a `ModuleScript` (or a path or asset id
/// that names one).
pub(crate) const INVALID: &str = "Attempted to call require with invalid argument(s).";

/// What one VM has required: each module's value, and the modules whose
/// body is still running (a `require` of one of those is a cycle).
#[derive(Default)]
pub(crate) struct Modules {
    loaded: HashMap<Ref, Value>,
    loading: HashSet<Ref>,
}

impl Modules {
    /// Forgets every module, so the next `require` runs its body again.
    pub(crate) fn clear(&mut self) {
        *self = Modules::default();
    }
}

/// Builds this VM's `require` factory. `resolve(script, target)` turns
/// `require`'s argument into the module instance (an error when it names
/// none); `compile(module)` gives the module's body as a function.
pub(crate) fn install(lua: &Lua, resolve: Function, compile: Function) -> Result<()> {
    let cached = lua.create_function(|lua, module: Value| {
        let module = module_ref(lua, &module)?;
        Ok(match engine(lua).modules.borrow().loaded.get(&module) {
            Some(value) => (true, value.clone()),
            None => (false, Value::Nil),
        })
    })?;
    let enter = lua.create_function(|lua, module: Value| {
        let module = module_ref(lua, &module)?;
        if engine(lua).modules.borrow_mut().loading.insert(module) {
            Ok(())
        } else {
            Err(mlua::Error::runtime(
                "Requested module was required recursively",
            ))
        }
    })?;
    let leave = lua.create_function(|lua, (module, value): (Value, Option<Value>)| {
        let module = module_ref(lua, &module)?;
        let engine = engine(lua);
        let mut modules = engine.modules.borrow_mut();
        modules.loading.remove(&module);
        if let Some(value) = value {
            modules.loaded.insert(module, value);
        }
        Ok(())
    })?;
    let made: Function = lua
        .load(
            r#"
            local resolve, compile, cached, enter, leave = ...
            return function(script)
                return function(target)
                    local module = resolve(script, target)
                    local hit, value = cached(module)
                    if hit then
                        return value
                    end
                    local body = compile(module)
                    enter(module)
                    local results = table.pack(pcall(body))
                    if not results[1] then
                        leave(module)
                        error(results[2], 0)
                    end
                    if results.n ~= 2 then
                        leave(module)
                        error("Module code did not return exactly one value", 2)
                    end
                    leave(module, results[2])
                    return results[2]
                end
            end
            "#,
        )
        .set_name("=require")
        .call((resolve, compile, cached, enter, leave))?;
    lua.set_named_registry_value(FACTORY, made)
}

/// A `require` bound to `script` (`nil` for code that has none, such as the
/// command bar).
pub(crate) fn require_for(lua: &Lua, script: Value) -> Result<Function> {
    lua.named_registry_value::<Function>(FACTORY)?.call(script)
}

/// The `ModuleScript` `value` is, or [`INVALID`].
pub(crate) fn module_ref(lua: &Lua, value: &Value) -> Result<Ref> {
    let invalid = || mlua::Error::runtime(INVALID);
    let Value::UserData(data) = value else {
        return Err(invalid());
    };
    let referent = data
        .borrow::<LuaInstance>()
        .map_err(|_| invalid())?
        .referent();
    let is_module = engine(lua)
        .ctx
        .dom()
        .get(referent)
        .is_some_and(|i| i.class() == "ModuleScript");
    if is_module {
        Ok(referent)
    } else {
        Err(invalid())
    }
}
