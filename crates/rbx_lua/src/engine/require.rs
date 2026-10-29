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

/// Where require-by-string's walk stands: the data model itself (`@game`,
/// or a service's parent) or an instance in it.
#[derive(Clone, Copy)]
enum At {
    Game,
    Instance(Ref),
}

/// The instance a require-by-string `path` names from `script`, walked the
/// way Luau's `Require` navigator (the library Roblox links) walks it:
/// `./` starts at `script.Parent`, `../` at `script.Parent.Parent`,
/// `@self` at `script` and `@game` at the data model; each other component
/// is a child by exact name, `..` a parent and `.` stays put. A failed step
/// gives the navigator's reason inside Roblox's `error requiring "…": …`.
pub(crate) fn by_string(dom: &rbx_dom::WeakDom, script: Option<Ref>, path: &str) -> Result<Ref> {
    let path = path.replace('\\', "/");
    let fail =
        |reason: String| mlua::Error::runtime(format!("error requiring \"{path}\": {reason}"));
    let parent = |at: At| match at {
        At::Instance(r) => Some(dom.parent(r).map_or(At::Game, At::Instance)),
        At::Game => None,
    };
    let requirer = || {
        script
            .map(At::Instance)
            .ok_or_else(|| fail("could not reset to requiring context".to_owned()))
    };
    let (mut at, rest) = if let Some(aliased) = path.strip_prefix('@') {
        let (alias, rest) = aliased.split_once('/').unwrap_or((aliased, ""));
        let start = match alias.to_ascii_lowercase().as_str() {
            "self" => requirer()?,
            "game" => {
                requirer()?;
                At::Game
            }
            other => return Err(fail(format!("@{other} is not a valid alias"))),
        };
        (start, rest)
    } else if path.starts_with("./") || path.starts_with("../") {
        let start = parent(requirer()?)
            .ok_or_else(|| fail("could not get parent of requiring context".to_owned()))?;
        (start, path.as_str())
    } else {
        return Err(fail(
            "require path must start with a valid prefix: ./, ../, or @".to_owned(),
        ));
    };
    let mut previous: Option<&str> = None;
    for component in rest.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                at = parent(at).ok_or_else(|| {
                    fail(match previous {
                        Some(name) => format!("could not get parent of component \"{name}\""),
                        None => "could not get parent of requiring context".to_owned(),
                    })
                })?;
            }
            name => {
                let child = match at {
                    At::Game => dom
                        .root_refs()
                        .iter()
                        .copied()
                        .find(|&r| dom.get(r).is_some_and(|i| i.name() == name)),
                    At::Instance(r) => crate::instance::tree::find_child(dom, r, name, false),
                };
                at = At::Instance(child.ok_or_else(|| {
                    fail(format!("could not resolve child component \"{name}\""))
                })?);
            }
        }
        previous = Some(component);
    }
    match at {
        At::Instance(found) if dom.get(found).is_some_and(|i| i.class() == "ModuleScript") => {
            Ok(found)
        }
        _ => Err(mlua::Error::runtime(INVALID)),
    }
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
