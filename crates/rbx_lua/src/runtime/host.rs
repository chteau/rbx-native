//! What the command bar plugs into the shared engine: its clock, output
//! and failure capture ([`CommandBar`]), the instance members it adds
//! ([`CommandBarHooks`]) and its `require`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, IntoLua, Lua, Result, Table, Thread, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::require::Requirer;
use crate::engine::{collection, engine, events, json, require, Host};
use crate::hooks::Hooks;
use crate::instance::tree::full_name;
use crate::instance::LuaInstance;
use crate::output::{LineKind, Output};
use crate::LuaError;

pub(super) struct CommandBar {
    output: Rc<RefCell<Output>>,
    clock: RefCell<Box<dyn Fn() -> Duration>>,
    next_id: Cell<u32>,
    /// Each pending run's main thread: an error there ends the run (it is
    /// the script's own error) rather than going to the output.
    pub(super) mains: RefCell<Vec<Thread>>,
    /// The first main-thread error since the last resume reported one.
    pub(super) failure: RefCell<Option<LuaError>>,
}

impl CommandBar {
    pub(super) fn new(output: Rc<RefCell<Output>>) -> Self {
        let started = Instant::now();
        CommandBar {
            output,
            clock: RefCell::new(Box::new(move || started.elapsed())),
            next_id: Cell::new(0),
            mains: RefCell::default(),
            failure: RefCell::default(),
        }
    }

    pub(super) fn set_clock(&self, clock: Box<dyn Fn() -> Duration>) {
        *self.clock.borrow_mut() = clock;
    }

    pub(super) fn fail(&self, error: LuaError) {
        self.failure.borrow_mut().get_or_insert(error);
    }
}

impl Host for CommandBar {
    fn now(&self) -> f64 {
        (self.clock.borrow())().as_secs_f64()
    }

    fn next_id(&self) -> u32 {
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }

    fn output(&self, kind: LineKind, text: String) {
        self.output.borrow_mut().push_text(kind, text);
    }

    fn thread_failed(&self, thread: &Thread, error: LuaError) {
        if self.mains.borrow().contains(thread) {
            self.fail(error);
        } else {
            self.output(LineKind::Error, error.to_string());
        }
    }
}

/// The members the command bar adds to instances: those both VMs share
/// (`engine::events`), `HttpService`'s offline half and
/// `CollectionService`. `game:GetService` finds or creates real services,
/// so these hang off real instances.
pub(super) struct CommandBarHooks;

impl Hooks for CommandBarHooks {
    fn index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>> {
        if let Some(value) = events::index(lua, ctx, referent, class, key)? {
            return Ok(Some(value));
        }
        match class {
            "HttpService" => http_member(lua, key),
            "CollectionService" => collection::member(lua, key),
            _ => Ok(None),
        }
    }

    fn new_index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
        value: &Value,
    ) -> Result<bool> {
        events::new_index(lua, ctx, referent, class, key, value)
    }

    fn service(&self, _lua: &Lua, _ctx: &Ctx, _name: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    fn game_member(&self, lua: &Lua, key: &str) -> Result<Option<Value>> {
        events::game_member(lua, key)
    }
}

/// JSON, GUIDs and `UrlEncode` work; the command bar has no network, so
/// the request methods fail saying so rather than pretending to send.
fn http_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    if let Some(member) = json::member(lua, key)? {
        return Ok(Some(member));
    }
    if !matches!(key, "GetAsync" | "PostAsync" | "RequestAsync") {
        return Ok(None);
    }
    let message = format!(
        "HttpService:{key} is not available from the command bar: it has no network access"
    );
    let f = lua.create_function(move |_, _: mlua::MultiValue| -> Result<()> {
        Err(mlua::Error::runtime(message.clone()))
    })?;
    Ok(Some(Value::Function(f)))
}

/// Installs the command bar's `require` (see `engine::require`): a
/// `ModuleScript` runs with its own `script` and `require`, and a string
/// path resolves against the requiring module (`require::by_string`). The
/// global one is the command bar's; code run over MCP gets its own (see
/// `report`).
pub(super) fn install_require(lua: &Lua) -> Result<()> {
    let resolve = lua.create_function(|lua, (script, target): (Value, Value)| {
        let module = match &target {
            Value::String(path) => {
                let path = path.to_str()?.to_string();
                require::by_string(&engine(lua).ctx.dom(), Requirer::of(&script), &path)?
            }
            other => require::module_ref(lua, other)?,
        };
        LuaInstance::new(module, engine(lua).ctx.clone()).into_lua(lua)
    })?;
    let compile = lua.create_function(|lua, module: Value| compile(lua, &module))?;
    require::install(lua, resolve, compile)?;
    let global = require::require_for(lua, require::COMMAND_BAR)?;
    lua.globals().set("require", global)
}

/// A module's `Source` as a function, named after the module's full name,
/// in an environment whose `script` is the module and whose `require`
/// resolves from it; other names fall through to the globals.
fn compile(lua: &Lua, module: &Value) -> Result<Function> {
    compile_in(lua, module, lua.globals())
}

/// [`compile`] with `fallback` in place of the globals: the test runner
/// puts `describe`, `it` and `expect` in front of them for a spec.
pub(crate) fn compile_in(lua: &Lua, module: &Value, fallback: Table) -> Result<Function> {
    let referent = require::module_ref(lua, module)?;
    let ctx = engine(lua).ctx.clone();
    let (source, name) = {
        let dom = ctx.dom();
        let instance = dom
            .get(referent)
            .ok_or_else(crate::property::missing_instance)?;
        let source = match instance.properties().get("Source") {
            Some(Variant::String(text)) => text.clone(),
            _ => String::new(),
        };
        let name = full_name(&dom, referent).unwrap_or_else(|| instance.name().to_string());
        (source, name)
    };
    let scope: Table = lua.create_table()?;
    scope.set("script", module.clone())?;
    scope.set("require", require::require_for(lua, module.clone())?)?;
    let meta = lua.create_table()?;
    meta.set("__index", fallback)?;
    scope.set_metatable(Some(meta))?;
    lua.load(source)
        .set_name(format!("={name}"))
        .set_environment(scope)
        .into_function()
}
