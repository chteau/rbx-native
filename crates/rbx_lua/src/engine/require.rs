//! `require` of a `ModuleScript`, built in Lua around a few Rust pieces so
//! a module body runs in a Lua frame: it may yield (`task.wait`, an HTTP
//! call) as in Roblox, which a module called from Rust could not.
//!
//! Each VM says how a `require` argument names a module and how a module's
//! `Source` becomes a function (see [`install`]); caching and the
//! recursion check are shared.
//!
//! With Luau exports on (see [`crate::set_luau_exports`]), a module that
//! `export`s values may be required again while its body still runs, the
//! way Studio's "Luau Exports and Cyclic Requires" beta allows: that
//! `require` gets a placeholder table that errors when touched, filled with
//! the module's exports once its body finishes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{Function, Lua, Result, Table, Value};
use rbx_dom::Ref;

use super::engine;
use crate::instance::LuaInstance;
use crate::output::LineKind;

const FACTORY: &str = "rbxn.require_factory";

/// Roblox's error for anything but a `ModuleScript` (or a path or asset id
/// that names one).
pub(crate) const INVALID: &str = "Attempted to call require with invalid argument(s).";

/// What the `require` of a module whose body failed raises, every time:
/// the body's own error goes to Output once, when it fails.
const FAILED: &str = "Requested module experienced an error while loading";

/// See [`crate::set_luau_exports`].
pub(crate) static EXPORTS: AtomicBool = AtomicBool::new(false);

/// The `script` to bind a `require` to for the command bar (see
/// [`Requirer::CommandBar`]).
pub(crate) const COMMAND_BAR: Value = Value::Boolean(true);

/// Who a `require` is bound to, which decides where a string path starts.
#[derive(Clone, Copy)]
pub(crate) enum Requirer {
    /// Nothing at all, as code run over MCP: every path fails with
    /// "could not reset to requiring context".
    Nothing,
    /// The command bar: a context of its own outside the tree, with no
    /// parent and no children, so only `@game` leads anywhere.
    CommandBar,
    Script(Ref),
}

impl Requirer {
    /// The requirer a `require` factory was bound with: a script instance,
    /// [`COMMAND_BAR`], or anything else for none.
    pub(crate) fn of(script: &Value) -> Requirer {
        match script {
            Value::UserData(data) => data
                .borrow::<LuaInstance>()
                .map_or(Requirer::Nothing, |i| Requirer::Script(i.referent())),
            Value::Boolean(true) => Requirer::CommandBar,
            _ => Requirer::Nothing,
        }
    }
}

/// What one VM has required: each module's value, the modules whose body
/// is still running (a `require` of one of those is a cycle), and the ones
/// whose body failed, which Roblox never runs again.
#[derive(Default)]
pub(crate) struct Modules {
    loaded: HashMap<Ref, Value>,
    loading: HashSet<Ref>,
    failed: HashSet<Ref>,
    /// The placeholder a cyclic `require` of a still-running module got.
    placeholders: HashMap<Ref, Table>,
}

/// Whether `source` exports values: an `export local`, `export const` or
/// `export function` statement, which Luau only allows at the top level.
fn exports_values(source: &str) -> bool {
    source.lines().any(|line| {
        line.trim_start()
            .strip_prefix("export")
            .is_some_and(|rest| {
                rest.starts_with(char::is_whitespace)
                    && ["local", "const", "function"]
                        .iter()
                        .any(|word| rest.trim_start().starts_with(word))
            })
    })
}

/// A table that raises Studio's cyclic-dependency errors when read or
/// written, and whose metatable is locked.
fn placeholder(lua: &Lua) -> Result<Table> {
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|_, (_, key): (Value, Value)| -> Result<()> {
            Err(mlua::Error::runtime(format!(
                "Cannot access the exported field '{}' because it has a cyclic dependency on its requiring module",
                key.to_string().unwrap_or_default()
            )))
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|_, (_, key, _): (Value, Value, Value)| -> Result<()> {
            Err(mlua::Error::runtime(format!(
                "Cannot set the exported field '{}' because it has a cyclic dependency on its requiring module",
                key.to_string().unwrap_or_default()
            )))
        })?,
    )?;
    meta.set("__metatable", "The metatable is locked")?;
    let table = lua.create_table()?;
    table.set_metatable(Some(meta))?;
    table.set_readonly(true);
    Ok(table)
}

/// Fills a placeholder with what the module returned, its metatable too,
/// and freezes it: everyone who required it, cycle or not, holds the same
/// table.
fn fill(placeholder: &Table, value: &Value) -> Result<()> {
    let Value::Table(result) = value else {
        return Err(mlua::Error::runtime(
            "Module code did not return a table of exports",
        ));
    };
    placeholder.set_readonly(false);
    for pair in result.pairs::<Value, Value>() {
        let (key, value) = pair?;
        placeholder.raw_set(key, value)?;
    }
    placeholder.set_metatable(result.metatable())?;
    placeholder.set_readonly(true);
    Ok(())
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
        let engine = engine(lua);
        let modules = engine.modules.borrow();
        if modules.failed.contains(&module) {
            return Err(mlua::Error::runtime(FAILED));
        }
        if let Some(value) = modules.loaded.get(&module) {
            return Ok((true, value.clone()));
        }
        if !modules.loading.contains(&module) || !EXPORTS.load(Ordering::Relaxed) {
            return Ok((false, Value::Nil));
        }
        let exports = engine.ctx.dom().get(module).is_some_and(|instance| {
            matches!(instance.properties().get("Source"),
                Some(rbx_dom::Variant::String(source)) if exports_values(source))
        });
        if !exports {
            return Ok((false, Value::Nil));
        }
        drop(modules);
        let mut modules = engine.modules.borrow_mut();
        let table = match modules.placeholders.get(&module) {
            Some(table) => table.clone(),
            None => {
                let table = placeholder(lua)?;
                modules.placeholders.insert(module, table.clone());
                table
            }
        };
        Ok((true, Value::Table(table)))
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
        let Some(value) = value else {
            modules.placeholders.remove(&module);
            return Ok(Value::Nil);
        };
        let value = match modules.placeholders.remove(&module) {
            Some(table) => {
                fill(&table, &value)?;
                Value::Table(table)
            }
            None => value,
        };
        modules.loaded.insert(module, value.clone());
        Ok(value)
    })?;
    // A body that failed: its error goes to Output and the module is never
    // run again, its `require` raising [`FAILED`] instead.
    let fail = lua.create_function(|lua, (module, error): (Value, Value)| {
        let module = module_ref(lua, &module)?;
        let engine = engine(lua);
        let text = match &error {
            Value::String(text) => text.to_string_lossy().to_string(),
            other => other.to_string()?,
        };
        engine.host.output(LineKind::Error, text);
        let mut modules = engine.modules.borrow_mut();
        modules.loading.remove(&module);
        modules.placeholders.remove(&module);
        modules.failed.insert(module);
        Err::<(), _>(mlua::Error::runtime(FAILED))
    })?;
    let made: Function = lua
        .load(
            r#"
            local resolve, compile, cached, enter, leave, fail = ...
            return function(script)
                return function(target)
                    local module = resolve(script, target)
                    local hit, value = cached(module)
                    if hit then
                        return value
                    end
                    -- A body that does not compile fails like one that
                    -- errors: Output gets the compiler's message.
                    local compiled, body = pcall(compile, module)
                    if not compiled then
                        fail(module, body)
                    end
                    enter(module)
                    local results = table.pack(pcall(body))
                    if not results[1] then
                        fail(module, results[2])
                    end
                    if results.n ~= 2 then
                        leave(module)
                        error("Module code did not return exactly one value", 0)
                    end
                    return leave(module, results[2])
                end
            end
            "#,
        )
        .set_name("=require")
        .call((resolve, compile, cached, enter, leave, fail))?;
    lua.set_named_registry_value(FACTORY, made)
}

/// A `require` bound to `script`: a script instance, [`COMMAND_BAR`], or
/// `nil` for code with no requiring context (see [`Requirer`]).
pub(crate) fn require_for(lua: &Lua, script: Value) -> Result<Function> {
    lua.named_registry_value::<Function>(FACTORY)?.call(script)
}

/// Where require-by-string's walk stands: the data model itself (`@game`,
/// or a service's parent), an instance in it, or the command bar's own
/// context, which has neither parent nor children.
#[derive(Clone, Copy)]
enum At {
    Game,
    Instance(Ref),
    CommandBar,
}

/// The instance a require-by-string `path` names from `requirer`, walked
/// the way Luau's `Require` navigator (the library Roblox links) walks it:
/// `./` starts at `script.Parent`, `../` at `script.Parent.Parent`,
/// `@self` at `script` and `@game` at the data model; each other component
/// is a child by exact name, `..` a parent and `.` stays put. A failed step
/// gives the navigator's reason inside Roblox's `error requiring "…": …`,
/// and a path that ends on anything but a `ModuleScript` gives Roblox's
/// `Unable to require module from given path '…'`.
pub(crate) fn by_string(dom: &rbx_dom::WeakDom, requirer: Requirer, path: &str) -> Result<Ref> {
    let path = path.replace('\\', "/");
    let fail =
        |reason: String| mlua::Error::runtime(format!("error requiring \"{path}\": {reason}"));
    let parent = |at: At| match at {
        At::Instance(r) => Some(dom.parent(r).map_or(At::Game, At::Instance)),
        At::Game | At::CommandBar => None,
    };
    let requirer = || match requirer {
        Requirer::Script(script) => Ok(At::Instance(script)),
        Requirer::CommandBar => Ok(At::CommandBar),
        Requirer::Nothing => Err(fail("could not reset to requiring context".to_owned())),
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
                    At::CommandBar => None,
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
        _ => Err(mlua::Error::runtime(format!(
            "Unable to require module from given path '{path}'"
        ))),
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
