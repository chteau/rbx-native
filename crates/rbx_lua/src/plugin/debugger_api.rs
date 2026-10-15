//! Debugger control for plugins — an rbx-native addition shaped after
//! Roblox's own `DebuggerManager`/`ScriptDebugger`, which real Studio only
//! exposes to the command bar.

use mlua::{Lua, Result, Value};

use super::api::DebuggerView;

/// `DebuggerManager()` (global) and `plugin:GetDebuggerManager()`.
/// Manager: `DebuggingEnabled` (true), `AddDebugger(script)` → script
/// debugger, `GetDebuggers()` (one per script with breakpoints),
/// `Resume()`, `StepIn()`, `StepOver()`, `StepOut()`, `Stop()`,
/// `Run(script)` (rbx-native), `DebuggerAdded` signal.
/// Script debugger: `Script`, `CurrentLine`, `IsDebugging`, `IsPaused`,
/// `SetBreakpoint(line, condition?)` → `{Line, Condition, Script}`,
/// `RemoveBreakpoint(line)`, `GetBreakpoints()`, `AddWatch(expr)` →
/// `{Expression}`, `RemoveWatch(expr)`, `GetWatches()`,
/// `GetWatchValue(watchOrExpr)`, `GetStack()` → `{{Function, Line}}`;
/// signals (`Owner::Instance(script)` keys) `EncounteredBreak(line)`,
/// `Resuming`, `BreakpointAdded(bp)`, `BreakpointRemoved(bp)`,
/// `WatchAdded(w)`, `WatchRemoved(w)`. Every action is a
/// `Request::Debugger`; reads come from `Context::debugger`. A native
/// plugin without `Permission::Debugger` gets an error.
pub(crate) fn manager(lua: &Lua) -> Result<Value> {
    todo!()
}

/// Installs the `DebuggerManager` global. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    todo!()
}

/// Fires the signals for what changed between two views (pause →
/// `EncounteredBreak`, unpause → `Resuming`, breakpoints/watches added or
/// removed).
pub(crate) fn changed(lua: &Lua, before: &DebuggerView, after: &DebuggerView) -> Result<()> {
    todo!()
}
