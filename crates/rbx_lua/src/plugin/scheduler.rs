//! Threads that wait: `task.wait`, `Signal:Wait`, HTTP, prompts.
//!
//! Every callback runs in its own Luau thread. A yielding binding is a Luau
//! wrapper (see [`yielding`]) that calls a Rust *start* function returning
//! an await id, then yields `("__await", id)`; the thread is parked under
//! that id until [`resolve`] resumes it with `(true, values…)` (the wrapper
//! returns the values) or `(false, message)` (the wrapper raises it).

use std::collections::{HashMap, VecDeque};

use mlua::{Function, Lua, MultiValue, Result, Thread};

use super::api::Arg;

pub(crate) type AwaitId = u32;

#[derive(Default)]
pub(crate) struct Scheduler {
    pub(crate) waiting: HashMap<AwaitId, Thread>,
    /// `(due, id, started)`: `task.wait`/`task.delay` timers, in seconds of
    /// `Context::now`.
    pub(crate) timers: Vec<(f64, AwaitId, f64)>,
    /// Threads to resume on the next tick (`task.defer`, a bare
    /// `coroutine.yield()` at a thread's top level), with their arguments.
    pub(crate) deferred: VecDeque<(Thread, MultiValue)>,
    /// Functions waiting on a timer instead of a thread (`task.delay`).
    pub(crate) delayed: HashMap<AwaitId, (Function, MultiValue)>,
}

/// Installs `task` (`spawn`, `defer`, `delay`, `wait`, `cancel`), `wait`,
/// `delay`, `spawn`, `tick`, `time`, `elapsedTime` as globals, and the
/// registry function [`yielding`] uses. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    todo!()
}

/// Wraps a Rust start function (`(...) -> AwaitId`) into the yielding Luau
/// function scripts call.
pub(crate) fn yielding(lua: &Lua, start: Function) -> Result<Function> {
    todo!()
}

/// Runs `func(args)` in a new thread now. An error is reported to the
/// Output as the plugin's (`OutputKind::Error`, message plus traceback),
/// never returned.
pub(crate) fn spawn(lua: &Lua, func: Function, args: MultiValue) -> Result<()> {
    todo!()
}

/// Resumes `thread` with `args` and files it by what it yields. Never
/// holds a borrow of `Env::scheduler` while Luau runs.
pub(crate) fn resume(lua: &Lua, thread: Thread, args: MultiValue) -> Result<()> {
    todo!()
}

/// A fresh await id (from `Shared::next_id`).
pub(crate) fn new_await(lua: &Lua) -> AwaitId {
    todo!()
}

/// Resumes the thread waiting on `id` (if any) with `(true, values…)` or
/// `(false, message)`.
pub(crate) fn resolve(lua: &Lua, id: AwaitId, result: std::result::Result<MultiValue, String>) -> Result<()> {
    todo!()
}

/// [`resolve`] with values built from [`Arg`]s on the VM's context.
pub(crate) fn resolve_args(lua: &Lua, id: AwaitId, result: std::result::Result<Vec<Arg>, String>) -> Result<()> {
    todo!()
}

/// An await id resolved by [`tick`] once `seconds` (at least one tick) have
/// passed, with the elapsed time.
pub(crate) fn sleep(lua: &Lua, seconds: f64) -> AwaitId {
    todo!()
}

/// Resolves due timers, runs `task.delay` functions whose time came, then
/// the deferred queue (only what was queued before this call).
pub(crate) fn tick(lua: &Lua, now: f64) -> Result<()> {
    todo!()
}

/// Whether [`tick`] at `now` would run anything.
pub(crate) fn wants_tick(scheduler: &Scheduler, now: f64) -> bool {
    todo!()
}
