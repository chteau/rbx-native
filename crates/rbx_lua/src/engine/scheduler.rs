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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Result, Thread, Value};

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
/// traceback taken where it was raised, not after the thread has unwound.
const RUNNER_SRC: &str = r#"
return function(f, ...)
    return xpcall(f, function(e) return debug.traceback(tostring(e), 2) end, ...)
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
        lua.create_function(|lua, seconds: Option<f64>| Ok(sleep(lua, seconds.unwrap_or(0.0))))?,
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
            engine(lua)
                .scheduler
                .borrow_mut()
                .deferred
                .push_back((thread.clone(), args));
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
            Ok(())
        })?,
    )?;

    globals.set("task", task)?;
    globals.set(
        "tick",
        lua.create_function(|_, ()| {
            Ok(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0))
        })?,
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

fn report(lua: &Lua, text: String) {
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
                engine(lua).host.thread_failed(&thread, error);
                return Ok(());
            }
        };
        if thread.status() != ThreadStatus::Resumable {
            // A runner thread finishes with `xpcall`'s results.
            let mut iter = values.iter();
            if let (Some(Value::Boolean(false)), message) = (iter.next(), iter.next()) {
                let text = match message {
                    Some(Value::String(s)) => s.to_string_lossy().to_string(),
                    Some(other) => format!("{other:?}"),
                    None => "error".to_string(),
                };
                report(lua, text);
            }
            return Ok(());
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
/// threads it resumed deferred in turn, Roblox's re-entrancy depth for
/// deferred events. A thread that keeps deferring itself waits for the
/// next tick past that.
const DEFER_ROUNDS: usize = 10;

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
            resume(lua, thread, args)?;
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
