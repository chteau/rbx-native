//! The scripting entry point.

use std::cell::{Ref as CellRef, RefCell};
use std::rc::Rc;

use mlua::chunk::Compiler;
use mlua::{Lua, Value, Variadic};
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use crate::ctx::Ctx;
use crate::debugger::{self, Breakpoint, Paused, Resume};
use crate::game::LuaGame;
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnums;
use crate::{datatypes, instance, LuaError};

/// Everything a script printed, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    lines: Vec<String>,
}

impl Output {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// A sandboxed Luau VM bound to one DOM.
///
/// The runtime owns the DOM while scripts run: bindings are mlua userdata, which
/// must own their state, so the tree is shared with them through
/// `Rc<RefCell<WeakDom>>`. Nothing here is `Send` (mlua is built without its
/// `send` feature), which keeps the borrow discipline a single-threaded one.
pub struct Runtime {
    lua: Lua,
    dom: Rc<RefCell<WeakDom>>,
    output: Rc<RefCell<Vec<String>>>,
}

impl Runtime {
    pub fn new(dom: WeakDom, database: ReflectionDatabase) -> Result<Self, LuaError> {
        let dom = Rc::new(RefCell::new(dom));
        let output = Rc::new(RefCell::new(Vec::new()));
        let ctx = Ctx::new(dom.clone(), Rc::new(database), output.clone());
        let lua = Lua::new();
        // Luau's import optimization resolves and caches `a.b.c` chains rooted at a
        // global once per chunk, which would make `workspace.Part.Size` return a
        // stale value after a script wrote to it. Declaring the DOM-backed globals
        // mutable turns that caching off for them, exactly as Roblox does.
        lua.set_compiler(Compiler::new().set_mutable_globals(["game", "workspace", "Workspace"]));

        install_globals(&lua, &ctx)?;
        // Globals must be installed first: sandboxing freezes them, which is the
        // point — one command-bar script cannot reshape the next one's environment.
        lua.sandbox(true)?;

        Ok(Runtime { lua, dom, output })
    }

    /// Runs a script to completion. Scripts are synchronous: there is no
    /// scheduler, so nothing can yield and no event can fire.
    pub fn run(&mut self, source: &str) -> Result<Output, LuaError> {
        self.output.borrow_mut().clear();
        self.lua.load(source).exec()?;
        Ok(Output {
            lines: std::mem::take(&mut self.output.borrow_mut()),
        })
    }

    /// Runs a script with the debugger attached: `on_pause` is called, on this
    /// thread and with the script frozen, whenever a breakpoint or a step
    /// request stops it, and says how to carry on. `name` is what the call
    /// stack and error messages call the chunk.
    pub fn debug(
        &mut self,
        source: &str,
        name: &str,
        breakpoints: &[Breakpoint],
        on_pause: impl FnMut(&Paused) -> Resume + 'static,
    ) -> Result<Output, LuaError> {
        self.output.borrow_mut().clear();
        debugger::run(
            &self.lua,
            source,
            name,
            breakpoints,
            self.output.clone(),
            on_pause,
        )?;
        Ok(Output {
            lines: std::mem::take(&mut self.output.borrow_mut()),
        })
    }

    /// Borrows the DOM the scripts have been mutating.
    ///
    /// Returns a guard rather than a plain `&WeakDom` because the tree is shared
    /// with the VM's userdata; it derefs to `WeakDom`.
    pub fn dom(&self) -> CellRef<'_, WeakDom> {
        self.dom.borrow()
    }

    pub fn into_dom(self) -> WeakDom {
        let Runtime { lua, dom, output } = self;
        // Closing the VM drops every userdata still holding a handle on the DOM,
        // which is what makes the unwrap below the normal path.
        drop(lua);
        drop(output);
        Rc::try_unwrap(dom)
            .map(RefCell::into_inner)
            .unwrap_or_else(|shared| shared.borrow().clone())
    }
}

fn install_globals(lua: &Lua, ctx: &Ctx) -> Result<(), LuaError> {
    let globals = lua.globals();

    let output_ctx = ctx.clone();
    globals.set(
        "print",
        lua.create_function(move |_, args: Variadic<Value>| {
            let mut parts = Vec::with_capacity(args.len());
            for value in args.iter() {
                parts.push(value.to_string()?);
            }
            output_ctx.print(parts.join(" "));
            Ok(())
        })?,
    )?;

    datatypes::register(lua)?;
    globals.set("Instance", instance::constructors(lua, ctx.clone())?)?;
    globals.set("Enum", LuaEnums::new(ctx.clone()))?;
    globals.set("game", LuaGame::new(ctx.clone()))?;

    let workspace = {
        let dom = ctx.dom();
        dom.root_refs()
            .iter()
            .copied()
            .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == "Workspace"))
    };
    if let Some(referent) = workspace {
        globals.set("workspace", LuaInstance::new(referent, ctx.clone()))?;
        globals.set("Workspace", LuaInstance::new(referent, ctx.clone()))?;
    }

    Ok(())
}
