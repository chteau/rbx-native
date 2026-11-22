//! Threads that wait: `task.wait`, `Signal:Wait`, HTTP, prompts.
//!
//! Every callback runs in its own Luau thread. A yielding binding is a Luau
//! wrapper (see [`yielding`]) that calls a Rust *start* function returning
//! an await id, then yields `("__await", id)`; the thread is parked under
//! that id until [`resolve`] resumes it with `(true, values…)` (the wrapper
//! returns the values) or `(false, message)` (the wrapper raises it).

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use std::ffi::c_int;

use mlua::thread::ThreadStatus;
use mlua::{ffi, Function, Lua, MultiValue, Result, Thread, Value};

use super::engine;
use crate::output::LineKind;

pub(crate) type AwaitId = u32;

const YIELDING_KEY: &str = "__rbxn_yielding";
const RUNNER_KEY: &str = "__rbxn_runner";

/// The running thread parks itself under its await id before yielding
/// (`park` hands back the result at once when it is already there), so a
/// wait works in any coroutine, one a script made with `coroutine.wrap`
/// included, as in Roblox, where the engine resumes whichever thread
/// waited, not whoever last resumed it.
const YIELDING_SRC: &str = r#"
local park = ...
return function(start)
    return function(...)
        local id = start(...)
        local ready, r = park(id, coroutine.running())
        if not ready then
            r = table.pack(coroutine.yield())
        end
        if r[1] then
            return table.unpack(r, 2, r.n)
        end
        error(r[2], 2)
    end
end
"#;

/// Runs a callback under `xpcall` so a failure arrives as a value with its
/// traceback taken where it was raised, not after the thread has unwound:
/// `(false, traceback, raised)`, `raised` being the error itself for the
/// thread's `coroutine.finally` callbacks.
const RUNNER_SRC: &str = r#"
return function(f, ...)
    local raised
    local r = table.pack(xpcall(f, function(e)
        raised = e
        return debug.traceback(tostring(e), 2)
    end, ...))
    if r[1] then
        return table.unpack(r, 1, r.n)
    end
    return false, r[2], raised
end
"#;

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
    /// The not-yet-started thread each `task.delay` handed back, so
    /// `task.cancel` can find its timer.
    pub(crate) delay_threads: Vec<(Thread, AwaitId)>,
    /// Ids resolved before their thread got as far as yielding on them.
    pub(crate) early: HashMap<AwaitId, std::result::Result<MultiValue, String>>,
    /// Each script's main thread: once the script is destroyed the thread
    /// never resumes, as a destroyed script stops in Roblox.
    pub(crate) owners: Vec<(Thread, rbx_dom::Ref)>,
    /// How deep in a chain of `task.defer` calls each queued thread is.
    defer_depth: HashMap<usize, u32>,
    /// The depth of the deferred thread running now, 0 for any other.
    depth: u32,
    /// The event whose handler is running now and how deep in a chain of
    /// handlers re-firing it (see `signal::EVENT_DEPTH`).
    pub(crate) event: Option<(super::signal::SignalKey, u32)>,
    /// The same for each queued deferred handler thread.
    pub(crate) event_depth: HashMap<usize, (super::signal::SignalKey, u32)>,
    /// The depth each event fired at in the last and in this round of
    /// instance events replayed from the change log.
    pub(crate) rounds: [HashMap<super::signal::SignalKey, u32>; 2],
}

impl Scheduler {
    /// When something is next due, on the host's clock: now for a deferred
    /// thread, else the earliest timer. `None` when nothing will ever wake
    /// on its own (threads waiting on a signal only wake when it fires).
    pub(crate) fn next_due(&self, now: f64) -> Option<f64> {
        if !self.deferred.is_empty() {
            return Some(now);
        }
        self.timers.iter().map(|(due, _, _)| *due).reduce(f64::min)
    }

    /// How many threads and callbacks are waiting to run.
    pub(crate) fn pending(&self) -> usize {
        self.waiting.len() + self.deferred.len() + self.delayed.len()
    }
}

/// Installs `task` (`spawn`, `defer`, `delay`, `wait`, `cancel`), `wait`,
/// `delay`, `spawn`, `tick`, `time`, `elapsedTime` as globals, and the
/// registry function [`yielding`] uses. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let park = lua.create_function(|lua, (id, thread): (AwaitId, Thread)| {
        let engine = engine(lua);
        let mut scheduler = engine.scheduler.borrow_mut();
        match scheduler.early.remove(&id) {
            Some(result) => {
                drop(scheduler);
                let values = resolution(lua, result)?;
                let packed = lua.create_sequence_from(values.iter().cloned())?;
                packed.set("n", values.len())?;
                Ok((true, Value::Table(packed)))
            }
            None => {
                scheduler.waiting.insert(id, thread);
                Ok((false, Value::Nil))
            }
        }
    })?;
    let maker: Function = lua.load(YIELDING_SRC).set_name("=yielding").call(park)?;
    lua.set_named_registry_value(YIELDING_KEY, maker)?;
    let runner_fn: Function = lua.load(RUNNER_SRC).set_name("=runner").eval()?;
    lua.set_named_registry_value(RUNNER_KEY, runner_fn)?;

    let globals = lua.globals();
    let task = lua.create_table()?;

    let wait = yielding(
        lua,
        lua.create_function(|lua, seconds: Value| {
            let seconds = match &seconds {
                Value::Nil => 0.0,
                Value::Integer(n) => *n as f64,
                Value::Number(n) => *n,
                other => other
                    .as_string()
                    .and_then(|s| s.to_str().ok()?.trim().parse::<f64>().ok())
                    .ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "invalid argument #1 to 'wait' (number expected, got {})",
                            other.type_name()
                        ))
                    })?,
            };
            Ok(sleep(lua, seconds))
        })?,
    )?;
    task.set("wait", wait.clone())?;
    globals.set("wait", wait)?;

    let spawn_fn = lua.create_function(|lua, (target, args): (Value, MultiValue)| {
        let thread = start_thread(lua, target, args)?;
        Ok(thread)
    })?;
    task.set("spawn", spawn_fn.clone())?;
    globals.set("spawn", spawn_fn)?;

    task.set(
        "defer",
        lua.create_function(|lua, (target, args): (Value, MultiValue)| {
            let (thread, args) = prepare(lua, target, args)?;
            let engine = engine(lua);
            let depth = engine.scheduler.borrow().depth + 1;
            // A chain of threads deferring the next stops at 80: the call
            // is reported (not raised) and the thread never runs (the
            // ScriptLimits golden).
            if depth > DEFER_DEPTH {
                engine.host.output(
                    LineKind::Error,
                    format!(
                        "Maximum re-entrancy depth ({DEFER_DEPTH}) exceeded calling task.defer\n{}",
                        roblox_stack(lua)
                    ),
                );
                return Ok(thread);
            }
            let mut scheduler = engine.scheduler.borrow_mut();
            scheduler
                .defer_depth
                .insert(thread.to_pointer() as usize, depth);
            scheduler.deferred.push_back((thread.clone(), args));
            Ok(thread)
        })?,
    )?;

    let delay_fn =
        lua.create_function(|lua, (seconds, func, args): (f64, Function, MultiValue)| {
            let thread = lua.create_thread(runner(lua)?)?;
            let id = sleep(lua, seconds);
            let engine = engine(lua);
            let mut scheduler = engine.scheduler.borrow_mut();
            scheduler.delayed.insert(id, (func, args));
            scheduler.delay_threads.push((thread.clone(), id));
            Ok(thread)
        })?;
    task.set("delay", delay_fn.clone())?;
    globals.set("delay", delay_fn)?;

    task.set(
        "cancel",
        lua.create_function(|lua, thread: Thread| {
            let engine = engine(lua);
            let mut scheduler = engine.scheduler.borrow_mut();
            scheduler.waiting.retain(|_, t| *t != thread);
            scheduler.deferred.retain(|(t, _)| *t != thread);
            if let Some(index) = scheduler
                .delay_threads
                .iter()
                .position(|(t, _)| *t == thread)
            {
                let (_, id) = scheduler.delay_threads.remove(index);
                scheduler.delayed.remove(&id);
                scheduler.timers.retain(|(_, timer, _)| *timer != id);
            }
            drop(scheduler);
            finalize(lua, &thread, End::Cancelled)
        })?,
    )?;

    globals.set("task", task)?;
    globals.set(
        "tick",
        lua.create_function(|lua, ()| Ok(crate::tape::clock(lua, crate::tape::epoch)))?,
    )?;
    let time = lua.create_function(|lua, ()| Ok(engine(lua).host.now()))?;
    globals.set("time", time.clone())?;
    globals.set("elapsedTime", time)?;
    Ok(())
}

fn runner(lua: &Lua) -> Result<Function> {
    lua.named_registry_value::<Function>(RUNNER_KEY)
}

/// A thread to run `target` with `args`: a function goes through the
/// runner (which then takes the function as its first argument), a thread
/// is resumed as it is.
fn prepare(lua: &Lua, target: Value, args: MultiValue) -> Result<(Thread, MultiValue)> {
    match target {
        Value::Function(func) => {
            let thread = lua.create_thread(runner(lua)?)?;
            let mut all = args;
            all.push_front(Value::Function(func));
            Ok((thread, all))
        }
        Value::Thread(thread) => Ok((thread, args)),
        other => Err(mlua::Error::runtime(format!(
            "invalid argument #1 (function or thread expected, got {})",
            other.type_name()
        ))),
    }
}

fn start_thread(lua: &Lua, target: Value, args: MultiValue) -> Result<Thread> {
    let (thread, args) = prepare(lua, target, args)?;
    resume(lua, thread.clone(), args)?;
    Ok(thread)
}

/// Wraps a Rust start function (`(...) -> AwaitId`) into the yielding Luau
/// function scripts call.
pub(crate) fn yielding(lua: &Lua, start: Function) -> Result<Function> {
    let maker: Function = lua.named_registry_value(YIELDING_KEY)?;
    maker.call(start)
}

/// Runs `func(args)` in a new thread now. An error goes to the host's
/// output (`LineKind::Error`, message plus traceback), never returned.
pub(crate) fn spawn(lua: &Lua, func: Function, args: MultiValue) -> Result<()> {
    start_thread(lua, Value::Function(func), args).map(|_| ())
}

/// Queues `func(args…)` for the next tick, as `task.defer` does.
pub(crate) fn defer(lua: &Lua, func: Function, args: MultiValue) -> Result<()> {
    let (thread, args) = prepare(lua, Value::Function(func), args)?;
    engine(lua)
        .scheduler
        .borrow_mut()
        .deferred
        .push_back((thread, args));
    Ok(())
}

/// [`defer`] for a handler of `event`, `depth` deep in a chain of handlers
/// re-firing it.
pub(crate) fn defer_handler(
    lua: &Lua,
    func: Function,
    args: MultiValue,
    event: (super::signal::SignalKey, u32),
) -> Result<()> {
    let (thread, args) = prepare(lua, Value::Function(func), args)?;
    let engine = engine(lua);
    let mut scheduler = engine.scheduler.borrow_mut();
    scheduler
        .event_depth
        .insert(thread.to_pointer() as usize, event);
    scheduler.deferred.push_back((thread, args));
    Ok(())
}

/// [`spawn`] for a handler of `event` (see [`defer_handler`]).
pub(crate) fn spawn_handler(
    lua: &Lua,
    func: Function,
    args: MultiValue,
    event: (super::signal::SignalKey, u32),
) -> Result<()> {
    let engine = engine(lua);
    let outer = engine.scheduler.borrow_mut().event.replace(event);
    let spawned = spawn(lua, func, args);
    engine.scheduler.borrow_mut().event = outer;
    spawned
}

/// Starts a new round of instance events replayed from the change log.
pub(crate) fn next_round(lua: &Lua) {
    let engine = engine(lua);
    let mut scheduler = engine.scheduler.borrow_mut();
    scheduler.rounds[0] = std::mem::take(&mut scheduler.rounds[1]);
}

/// Forgets the rounds, at the start of a resumption point.
pub(crate) fn reset_rounds(lua: &Lua) {
    engine(lua).scheduler.borrow_mut().rounds = Default::default();
}

/// The script timeout's deadline, which the VM's interrupt reads: as in
/// Roblox, each resumption of a script thread gets the whole budget, and a
/// thread that runs past it dies alone, the threads after it still run.
pub(crate) struct Deadline {
    at: Rc<Cell<Instant>>,
    budget: Duration,
}

impl Deadline {
    /// Arms `budget` from now; the interrupt reads the returned cell.
    pub(crate) fn arm(lua: &Lua, budget: Duration) -> Rc<Cell<Instant>> {
        let at = Rc::new(Cell::new(Instant::now() + budget));
        lua.set_app_data(Deadline {
            at: at.clone(),
            budget,
        });
        at
    }

    /// Runs `f`, a thread's resumption, with the whole budget, and gives
    /// what runs after it (the thread that resumed it, the scheduler) a
    /// fresh one: a thread the timeout killed doesn't take its resumer down
    /// with it.
    fn fresh<T>(lua: &Lua, f: impl FnOnce() -> T) -> T {
        let Some((at, budget)) = lua
            .app_data_ref::<Deadline>()
            .map(|d| (d.at.clone(), d.budget))
        else {
            return f();
        };
        at.set(Instant::now() + budget);
        let out = f();
        at.set(Instant::now() + budget);
        out
    }
}

/// The engine's own Luau chunks: a traceback names scripts only, as
/// Roblox's does.
const INTERNAL_CHUNKS: &[&str] = &[
    "errors", "runner", "run", "yielding", "wait", "watch", "require", "alias", "getfenv",
    "services",
];

/// Whether `chunk` is one of the engine's own (see [`INTERNAL_CHUNKS`]) or
/// mlua's.
pub(crate) fn internal_chunk(chunk: &str) -> bool {
    INTERNAL_CHUNKS.contains(&chunk) || chunk.starts_with("__mlua")
}

fn report(lua: &Lua, text: String) {
    let text = text
        .lines()
        .filter(|line| {
            !line.split_once(':').is_some_and(|(chunk, rest)| {
                internal_chunk(chunk) && rest.starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    engine(lua).host.output(LineKind::Error, text.clone());
    script_error(lua, &text);
}

/// `ScriptContext.Error` for an error a thread died of: the message, then
/// the traceback, and the script (which we don't track per thread: nil).
fn script_error(lua: &Lua, text: &str) {
    thread_local! {
        /// An `Error` handler's own error isn't fired again.
        static FIRING: Cell<bool> = const { Cell::new(false) };
    }
    if FIRING.with(Cell::get) {
        return;
    }
    let context = {
        let ctx = engine(lua).ctx.clone();
        let dom = ctx.dom();
        dom.root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "ScriptContext"))
    };
    let Some(context) = context else {
        return;
    };
    let (message, trace) = text.split_once('\n').unwrap_or((text, ""));
    let args = [
        crate::plugin::api::Arg::String(message.to_owned()),
        crate::plugin::api::Arg::String(trace.to_owned()),
        crate::plugin::api::Arg::Nil,
    ];
    let key = super::events::instance_key(context, "Error");
    FIRING.with(|f| f.set(true));
    let fired = super::signal::fire(lua, &key, &args);
    FIRING.with(|f| f.set(false));
    if let Err(error) = fired {
        engine(lua).host.output(LineKind::Error, error.to_string());
    }
}

/// Resumes `thread` with `args` and files it by what it yields. Never
/// holds a borrow of `Env::scheduler` while Luau runs.
/// Whether `thread` is the main thread of a script that was destroyed.
fn orphaned(lua: &Lua, thread: &Thread) -> bool {
    let engine = engine(lua);
    let mut scheduler = engine.scheduler.borrow_mut();
    scheduler
        .owners
        .retain(|(t, _)| t.status() == ThreadStatus::Resumable);
    let Some(script) = scheduler
        .owners
        .iter()
        .find(|(t, _)| t == thread)
        .map(|o| o.1)
    else {
        return false;
    };
    drop(scheduler);
    let gone = engine.ctx.dom().get(script).is_none();
    gone
}

pub(crate) fn resume(lua: &Lua, thread: Thread, args: MultiValue) -> Result<()> {
    if orphaned(lua, &thread) {
        return Ok(());
    }
    let mut args = args;
    loop {
        if thread.status() != ThreadStatus::Resumable {
            return Ok(());
        }
        let values = match Deadline::fresh(lua, || thread.resume::<MultiValue>(args)) {
            Ok(values) => values,
            Err(error) => {
                let message = lua.create_string(crate::runtime::globals::error_text(&error))?;
                engine(lua).host.thread_failed(&thread, error);
                return finalize(lua, &thread, End::Failed(Value::String(message)));
            }
        };
        if thread.status() != ThreadStatus::Resumable {
            // A runner thread finishes with `xpcall`'s results.
            let mut values = values;
            let end = match values.front() {
                Some(Value::Boolean(false)) => {
                    let message = values.get(1).cloned().unwrap_or(Value::Nil);
                    let raised = values.get(2).cloned().unwrap_or_else(|| message.clone());
                    let text = match &message {
                        Value::String(s) => s.to_string_lossy().to_string(),
                        Value::Nil => "error".to_string(),
                        other => format!("{other:?}"),
                    };
                    report(lua, text);
                    End::Failed(raised)
                }
                Some(Value::Boolean(true)) => {
                    values.pop_front();
                    End::Finished(values)
                }
                _ => End::Finished(values),
            };
            return finalize(lua, &thread, end);
        }
        let awaited = match (values.front(), values.get(1)) {
            (Some(Value::String(tag)), Some(id)) if tag.as_bytes() == b"__await" => match id {
                Value::Integer(i) => Some(*i as AwaitId),
                Value::Number(n) => Some(*n as AwaitId),
                _ => None,
            },
            _ => None,
        };
        let engine = engine(lua);
        let mut scheduler = engine.scheduler.borrow_mut();
        match awaited {
            Some(id) => match scheduler.early.remove(&id) {
                Some(result) => {
                    drop(scheduler);
                    args = resolution(lua, result)?;
                }
                None => {
                    scheduler.waiting.insert(id, thread);
                    return Ok(());
                }
            },
            // A plain `coroutine.yield()` suspends the thread until
            // whoever holds it resumes it, as in Roblox, signal libraries
            // park their runner threads exactly this way.
            None => return Ok(()),
        }
    }
}

/// How a scheduler thread ended, for its `coroutine.finally` callbacks.
enum End {
    Finished(MultiValue),
    Failed(Value),
    Cancelled,
}

/// Runs `thread`'s `coroutine.finally` callbacks, newest first, in a
/// thread of their own, as Roblox does for the threads its scheduler runs:
/// `("finished", results…)`, `("error", message)` (the finalizer then
/// raises the error again, so Output shows it twice, as Studio's does) or
/// `("cancelled")` for `task.cancel`. The callbacks may yield.
fn finalize(lua: &Lua, thread: &Thread, end: End) -> Result<()> {
    unsafe extern "C-unwind" fn raise(state: *mut ffi::lua_State) -> c_int {
        ffi::lua_error(state)
    }
    let (cancelled, failed, mut args) = match end {
        End::Finished(values) => (false, false, values),
        End::Failed(message) => (false, true, MultiValue::from_vec(vec![message])),
        End::Cancelled => (true, false, MultiValue::new()),
    };
    args.push_front(Value::Thread(thread.clone()));
    // SAFETY: Luau C API calls on mlua's state and on the argument thread,
    // each checked against that thread's status first; none unwinds (the
    // one error raised is caught by `lua_resume`).
    let finalizer = unsafe {
        lua.exec_raw::<Option<Function>>(args, |state| {
            let co = ffi::lua_tothread(state, 1);
            let payload = ffi::lua_gettop(state) - 1;
            let runnable = !co.is_null()
                && ffi::lua_hasfinalizers(co) != 0
                && if cancelled {
                    ffi::lua_costatus(state, co) == ffi::LUA_COSUS
                } else {
                    ffi::lua_status(co) != ffi::LUA_YIELD
                };
            if !runnable {
                ffi::lua_settop(state, 0);
                return;
            }
            if cancelled {
                ffi::lua_pushfinalizerfunction(state, co, 1);
            } else {
                ffi::lua_settop(co, 0);
                if failed && ffi::lua_status(co) == ffi::LUA_OK {
                    // A runner thread caught its error under `xpcall` and
                    // ended well: end it again by raising the message, so
                    // the callbacks are told it failed.
                    ffi::lua_pushcfunction(co, raise);
                    ffi::lua_xmove(state, co, 1);
                    ffi::lua_resume_(co, state, 1);
                } else {
                    ffi::lua_xmove(state, co, payload);
                }
                ffi::lua_pushfinalizerfunction(state, co, 0);
            }
            ffi::lua_replace(state, 1);
            ffi::lua_settop(state, 1);
        })?
    };
    match finalizer {
        Some(finalizer) => {
            start_thread(lua, Value::Function(finalizer), MultiValue::new()).map(drop)
        }
        None => Ok(()),
    }
}

fn resolution(lua: &Lua, result: std::result::Result<MultiValue, String>) -> Result<MultiValue> {
    Ok(match result {
        Ok(mut values) => {
            values.push_front(Value::Boolean(true));
            values
        }
        Err(message) => MultiValue::from_vec(vec![
            Value::Boolean(false),
            Value::String(lua.create_string(&message)?),
        ]),
    })
}

/// A fresh await id (from `Host::next_id`).
pub(crate) fn new_await(lua: &Lua) -> AwaitId {
    engine(lua).host.next_id()
}

/// Resumes the thread waiting on `id` (if any) with `(true, values…)` or
/// `(false, message)`.
pub(crate) fn resolve(
    lua: &Lua,
    id: AwaitId,
    result: std::result::Result<MultiValue, String>,
) -> Result<()> {
    let engine = engine(lua);
    let thread = {
        let mut scheduler = engine.scheduler.borrow_mut();
        match scheduler.waiting.remove(&id) {
            Some(thread) => thread,
            None => {
                scheduler.early.insert(id, result);
                return Ok(());
            }
        }
    };
    let args = resolution(lua, result)?;
    resume(lua, thread, args)
}

/// An await id resolved by [`tick`] once `seconds` (at least one tick) have
/// passed, with the elapsed time.
pub(crate) fn sleep(lua: &Lua, seconds: f64) -> AwaitId {
    let id = new_await(lua);
    let engine = engine(lua);
    let now = engine.host.now();
    let seconds = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    engine
        .scheduler
        .borrow_mut()
        .timers
        .push((now + seconds, id, now));
    id
}

/// How many times one [`tick`] goes back to the deferred queue for what the
/// threads it resumed deferred in turn: a whole `task.defer` chain runs in
/// one frame on a real server (ScriptLimits: 80 in the first frame). A
/// thread that keeps yielding at its top level waits for the next tick
/// past that.
const DEFER_ROUNDS: usize = DEFER_DEPTH as usize + 1;

/// How long a chain of `task.defer` calls (each from a thread the last one
/// deferred) may grow.
const DEFER_DEPTH: u32 = 80;

/// The running thread's script frames, innermost first: the script's full
/// name, the line, the function's name.
pub(crate) fn script_frames(lua: &Lua) -> Vec<(String, Option<usize>, Option<String>)> {
    let mut frames = Vec::new();
    for level in 1.. {
        let Some(frame) = lua.inspect_stack(level, |debug| {
            let source = debug.source();
            let name = debug.names().name.map(|n| n.into_owned());
            (
                source.what,
                source.source.map(|s| s.into_owned()).unwrap_or_default(),
                debug.current_line(),
                name,
            )
        }) else {
            break;
        };
        let (what, source, line, name) = frame;
        let Some(script) = source.strip_prefix('=') else {
            continue;
        };
        if what == "C" || internal_chunk(script) {
            continue;
        }
        frames.push((script.to_owned(), line, name));
    }
    frames
}

/// The running thread's stack as Roblox lays one out under an engine
/// error: `Script '<full name>', Line N - function f` per Luau frame.
pub(crate) fn roblox_stack(lua: &Lua) -> String {
    let mut lines = vec!["    Stack Begin".to_owned()];
    for (script, line, name) in script_frames(lua) {
        let mut text = format!("    Script '{script}', Line {}", line.unwrap_or(0));
        if let Some(name) = name {
            text += &format!(" - function {name}");
        }
        lines.push(text);
    }
    lines.push("    Stack End".to_owned());
    lines.join("\n")
}

/// Resolves due timers, runs `task.delay` functions whose time came, then
/// the deferred queue, as Roblox ends a resumption cycle: what those
/// threads defer runs in the same tick, before any later wait is over.
pub(crate) fn tick(lua: &Lua, now: f64) -> Result<()> {
    let engine = engine(lua);
    engine.host.set_now(now);
    let frame_clock = engine.host.frame_clock();
    let mut due = {
        let mut scheduler = engine.scheduler.borrow_mut();
        let (due, rest): (Vec<_>, Vec<_>) = scheduler
            .timers
            .drain(..)
            // On a frame clock a wait is over on the first frame strictly
            // past its time (the WaitRounding golden: task.wait(0.5) takes
            // 31 frames, wait() 2, task.wait(0) 1).
            .partition(|&(at, _, _)| {
                if frame_clock {
                    now > at + 1e-9
                } else {
                    at <= now
                }
            });
        scheduler.timers = rest;
        due
    };
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, id, started) in due {
        let delayed = {
            let mut scheduler = engine.scheduler.borrow_mut();
            scheduler.delayed.remove(&id).map(|call| {
                let thread = scheduler
                    .delay_threads
                    .iter()
                    .position(|(_, timer)| *timer == id)
                    .map(|index| scheduler.delay_threads.remove(index).0);
                (call, thread)
            })
        };
        match delayed {
            Some(((func, mut args), thread)) => {
                let thread = match thread {
                    Some(thread) => thread,
                    None => lua.create_thread(runner(lua)?)?,
                };
                args.push_front(Value::Function(func));
                resume(lua, thread, args)?;
            }
            None => resolve(
                lua,
                id,
                Ok(MultiValue::from_vec(vec![
                    Value::Number(now - started),
                    Value::Number(now),
                ])),
            )?,
        }
    }
    run_deferred(lua)
}

/// Runs the deferred queue (`task.defer`, deferred signal handlers),
/// including what those calls defer, up to `DEFER_ROUNDS` rounds.
pub(crate) fn run_deferred(lua: &Lua) -> Result<()> {
    let engine = engine(lua);
    let mut deferred: Vec<_> = engine.scheduler.borrow_mut().deferred.drain(..).collect();
    for _ in 0..DEFER_ROUNDS {
        if deferred.is_empty() {
            break;
        }
        for (thread, args) in deferred {
            let key = thread.to_pointer() as usize;
            {
                let mut scheduler = engine.scheduler.borrow_mut();
                scheduler.depth = scheduler.defer_depth.remove(&key).unwrap_or(0);
                scheduler.event = scheduler.event_depth.remove(&key);
            }
            let resumed = resume(lua, thread, args);
            let mut scheduler = engine.scheduler.borrow_mut();
            scheduler.depth = 0;
            scheduler.event = None;
            drop(scheduler);
            resumed?;
        }
        deferred = engine.scheduler.borrow_mut().deferred.drain(..).collect();
    }
    let mut scheduler = engine.scheduler.borrow_mut();
    for call in deferred.into_iter().rev() {
        scheduler.deferred.push_front(call);
    }
    Ok(())
}

/// Whether [`tick`] at `now` would run anything.
pub(crate) fn wants_tick(scheduler: &Scheduler, now: f64) -> bool {
    !scheduler.deferred.is_empty() || scheduler.timers.iter().any(|(due, _, _)| *due <= now)
}

#[cfg(test)]
mod tests;
