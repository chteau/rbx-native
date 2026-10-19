//! The scripting entry point.

mod globals;
mod host;
mod resume;
#[cfg(test)]
mod tests;

use std::cell::{Ref as CellRef, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mlua::chunk::Compiler;
use mlua::Lua;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use crate::ctx::Ctx;
use crate::debugger::{self, Breakpoint, Paused, Resume};
use crate::engine::{self, Engine};
use crate::hooks::HookSlot;
use crate::output::{LineKind, Output};
use crate::LuaError;

pub(crate) use globals::install_globals;
use host::{CommandBar, CommandBarHooks};

/// How long [`Runtime::run`] waits on a script's sleeping threads before
/// abandoning them.
const RUN_LIMIT: Duration = Duration::from_secs(30);

/// What one [`Runtime::resume`] did: the lines printed during it, and where
/// the runtime stands.
#[derive(Debug, Clone)]
pub struct Step {
    pub output: Output,
    pub state: RunState,
}

#[derive(Debug, Clone)]
pub enum RunState {
    /// Nothing is left to run: every thread returned, or waits on a signal
    /// that only another run could fire.
    Finished,
    /// A thread sleeps (`task.wait`, `task.delay`) or was deferred: resume
    /// again once `wake_in` has passed.
    Pending { wake_in: Duration },
    /// A run's main chunk raised this error; every other thread was
    /// cancelled with it.
    Failed(LuaError),
}

/// A script that failed, with everything it printed before it did (boxed
/// to keep `Result`s small).
#[derive(Debug, Clone)]
pub struct RunError {
    pub output: Box<Output>,
    pub error: LuaError,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for RunError {}

/// A sandboxed Luau VM bound to one DOM, run the way Studio's command bar
/// runs code: each chunk in a thread of its own that may yield, with
/// `task`, signals, instance events and `require` working as in Roblox.
///
/// Bindings are mlua userdata, which must own their state, so the tree is
/// shared with them through `Rc<RefCell<WeakDom>>`. The runtime either owns
/// it ([`Runtime::new`], [`Runtime::run`]) or is lent the host's for each
/// step ([`Runtime::lent`], [`Runtime::resume_in`]), so the editor keeps
/// the tree between steps; it is the same tree each time, so every `Ref`
/// stays valid. Nothing here is `Send` (mlua is built without its `send`
/// feature), which keeps the borrow discipline a single-threaded one.
pub struct Runtime {
    lua: Lua,
    dom: Rc<RefCell<WeakDom>>,
    output: Rc<RefCell<Output>>,
    host: Rc<CommandBar>,
    /// How long one resume lets scripts go before the running thread
    /// errors, so a `while true do end` cannot freeze the editor for good.
    budget: Duration,
}

impl Runtime {
    pub fn new(dom: WeakDom, database: ReflectionDatabase) -> Result<Self, LuaError> {
        Self::build(Rc::new(RefCell::new(dom)), database)
    }

    /// A runtime over a DOM the host keeps: `dom` is read while the VM is
    /// built (for `workspace`) and left with the caller, who lends it to
    /// each [`Runtime::resume_in`].
    pub fn lent(dom: &mut WeakDom, database: ReflectionDatabase) -> Result<Self, LuaError> {
        let slot = Rc::new(RefCell::new(std::mem::take(dom)));
        let built = Self::build(slot.clone(), database);
        std::mem::swap(dom, &mut slot.borrow_mut());
        built
    }

    fn build(dom: Rc<RefCell<WeakDom>>, database: ReflectionDatabase) -> Result<Self, LuaError> {
        let output = Rc::new(RefCell::new(Output::default()));
        let ctx = Ctx::new(dom.clone(), Rc::new(database), output.clone());
        let host = Rc::new(CommandBar::new(output.clone()));
        let lua = Lua::new();
        // Luau's import optimization resolves and caches `a.b.c` chains rooted at a
        // global once per chunk, which would make `workspace.Part.Size` return a
        // stale value after a script wrote to it. Declaring the DOM-backed globals
        // mutable turns that caching off for them, exactly as Roblox does.
        lua.set_compiler(Compiler::new().set_mutable_globals(["game", "workspace", "Workspace"]));
        lua.set_app_data(HookSlot(Rc::new(CommandBarHooks)));

        install_globals(&lua, &ctx)?;
        engine::install(&lua, Engine::new(ctx, host.clone()))?;
        host::install_require(&lua)?;
        // Globals must be installed first: sandboxing freezes them, which is the
        // point — one command-bar script cannot reshape the next one's environment.
        lua.sandbox(true)?;

        Ok(Runtime {
            lua,
            dom,
            output,
            host,
            budget: Duration::from_secs(10),
        })
    }

    /// Replaces the clock timers and `time()` read: the time since the
    /// runtime was made, by default.
    pub fn set_clock(&mut self, clock: impl Fn() -> Duration + 'static) {
        self.host.set_clock(Box::new(clock));
    }

    /// Runs a script and every thread it starts to completion, sleeping
    /// while they wait, on the runtime's own DOM. Threads still waiting
    /// after 30 seconds are abandoned, with a warning in the output.
    pub fn run(&mut self, source: &str) -> Result<Output, RunError> {
        self.run_for(source, RUN_LIMIT)
    }

    /// [`Runtime::run`], abandoning threads that would still wait after
    /// `limit` (counted on the wall clock).
    pub fn run_for(&mut self, source: &str, limit: Duration) -> Result<Output, RunError> {
        let started = Instant::now();
        self.start(source);
        let mut output = Output::default();
        loop {
            let step = self.resume();
            output.extend(step.output);
            match step.state {
                RunState::Finished => return Ok(output),
                RunState::Failed(error) => {
                    return Err(RunError {
                        output: Box::new(output),
                        error,
                    })
                }
                RunState::Pending { wake_in } => {
                    if started.elapsed() + wake_in > limit {
                        let left = engine::engine(&self.lua).scheduler.borrow().pending();
                        self.cancel();
                        output.push_text(
                            LineKind::Warn,
                            format!(
                                "{left} thread(s) still waiting after {:.0} s were abandoned",
                                limit.as_secs_f64()
                            ),
                        );
                        return Ok(output);
                    }
                    std::thread::sleep(wake_in);
                }
            }
        }
    }

    /// Runs a script with the debugger attached: `on_pause` is called, on this
    /// thread and with the script frozen, whenever a breakpoint or a step
    /// request stops it, and says how to carry on. `name` is what the call
    /// stack and error messages call the chunk. Setting `stop`, from any
    /// thread, stops the script as [`Resume::Stop`] would, paused or not.
    /// The chunk runs synchronously: it cannot yield.
    pub fn debug(
        &mut self,
        source: &str,
        name: &str,
        breakpoints: &[Breakpoint],
        stop: Arc<AtomicBool>,
        on_pause: impl FnMut(&Paused) -> Resume + 'static,
    ) -> Result<Output, RunError> {
        *self.output.borrow_mut() = Output::default();
        let ran = debugger::run(
            &self.lua,
            source,
            name,
            breakpoints,
            self.output.clone(),
            stop,
            on_pause,
        );
        let output = std::mem::take(&mut *self.output.borrow_mut());
        match ran {
            Ok(()) => Ok(output),
            Err(error) => Err(RunError {
                output: Box::new(output),
                error,
            }),
        }
    }

    /// Borrows the DOM the scripts have been mutating.
    ///
    /// Returns a guard rather than a plain `&WeakDom` because the tree is shared
    /// with the VM's userdata; it derefs to `WeakDom`.
    pub fn dom(&self) -> CellRef<'_, WeakDom> {
        self.dom.borrow()
    }

    pub fn into_dom(self) -> WeakDom {
        let Runtime {
            lua,
            dom,
            output,
            host,
            budget: _,
        } = self;
        // Closing the VM drops every userdata still holding a handle on the DOM,
        // which is what makes the unwrap below the normal path.
        drop(lua);
        drop(output);
        drop(host);
        Rc::try_unwrap(dom)
            .map(RefCell::into_inner)
            .unwrap_or_else(|shared| shared.borrow().clone())
    }
}
