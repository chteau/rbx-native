//! Line breakpoints and stepping, over the debug hooks Luau has in place of
//! `debug.sethook`.
//!
//! A debug run pays for the debugger only where it is used:
//!
//! - **Breakpoints** are Luau's own: `lua_breakpoint` patches every
//!   instruction on the line into `LOP_BREAK`, so the VM calls back
//!   (`debugbreak`) on those lines and nowhere else.
//! - **Stepping** needs a call before every instruction (`debugstep`). The
//!   VM only makes those calls when the thread was entered in single-step
//!   mode, and that mode cannot be switched on mid-run, so a run always
//!   starts in it — which costs little on its own — and the per-instruction
//!   callback is installed only while a step is in progress or a
//!   breakpoint line is being watched (see `hook`).
//! - **Stop** is checked in the `interrupt` callback, which Luau calls at
//!   every call, return and loop back-edge, so it also ends a script that
//!   never reaches a breakpoint.
//!
//! Whenever the script pauses, the caller's `on_pause` runs synchronously on
//! the script's own thread — that is what "paused" means here. Whatever it
//! does before returning (block on a channel, evaluate watches, walk the
//! call stack) happens with the script frozen mid-line.

mod frame;
mod hook;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use mlua::chunk::Compiler;
use mlua::Lua;

pub use frame::{Frame, Paused, Variable};

/// One line breakpoint, configured the way Studio's Edit Breakpoint window
/// configures one: a standard breakpoint has neither a condition nor a log
/// message, a logpoint is a log message with `continue_execution` set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breakpoint {
    /// 1-based, as the gutter numbers lines.
    pub line: u32,
    /// Only activates while this expression is truthy. An expression that
    /// errors activates it, with the error logged, rather than silently
    /// never stopping.
    pub condition: Option<String>,
    /// Printed to the output as `print(<log_message>)` would print it.
    pub log_message: Option<String>,
    /// Log (if there is a message) without pausing.
    pub continue_execution: bool,
}

/// What `on_pause` tells the paused script to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Run until the next breakpoint.
    Continue,
    /// Stop at the next line reached, including inside a called function.
    StepInto,
    /// Stop at the next line of this function or of a caller.
    StepOver,
    /// Stop once this function has returned.
    StepOut,
    /// Abandon the script with an error; the DOM keeps what it already did.
    Stop,
}

/// The message a script stopped from the debugger fails with.
pub const STOPPED: &str = "Script stopped by the debugger";

/// Runs `source` with the debugger attached. The chunk is compiled with full
/// debug info and no optimization, so every line and every local survives
/// to be inspected, and with statement coverage, which is how a pass through
/// a one-line loop is told from the next (see `hook`).
pub(crate) fn run(
    lua: &Lua,
    source: &str,
    name: &str,
    breakpoints: &[Breakpoint],
    output: Rc<RefCell<Vec<String>>>,
    stop: Arc<AtomicBool>,
    on_pause: impl FnMut(&Paused) -> Resume + 'static,
) -> mlua::Result<()> {
    let chunk = lua
        .load(source)
        .set_name(format!("={name}"))
        .set_compiler(debug_compiler())
        .into_function()?;
    let _attached = hook::attach(lua, &chunk, breakpoints, output, stop, Box::new(on_pause))?;
    chunk.call::<()>(())
}

fn debug_compiler() -> Compiler {
    Compiler::new()
        .set_optimization_level(0)
        .set_debug_level(2)
        .set_coverage_level(1)
        .set_mutable_globals(["game", "workspace", "Workspace"])
}
