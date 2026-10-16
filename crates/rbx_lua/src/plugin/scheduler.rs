//! Threads that wait: `task.wait`, `Signal:Wait`, HTTP, prompts.
//!
//! Every callback runs in its own Luau thread. A yielding binding is a Luau
//! wrapper (see [`yielding`]) that calls a Rust *start* function returning
//! an await id, then yields `("__await", id)`; the thread is parked under
//! that id until [`resolve`] resumes it with `(true, values…)` (the wrapper
//! returns the values) or `(false, message)` (the wrapper raises it).

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Result, Thread, Value};

use super::api::OutputKind;
use super::env::env;

pub(crate) type AwaitId = u32;

const YIELDING_KEY: &str = "__plugin_yielding";
const RUNNER_KEY: &str = "__plugin_runner";

/// The running thread parks itself under its await id before yielding
/// (`park` hands back the result at once when it is already there), so a
/// wait works in any coroutine — one a script made with `coroutine.wrap`
/// included — as in Roblox, where the engine resumes whichever thread
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
}

/// Installs `task` (`spawn`, `defer`, `delay`, `wait`, `cancel`), `wait`,
/// `delay`, `spawn`, `tick`, `time`, `elapsedTime` as globals, and the
/// registry function [`yielding`] uses. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let park = lua.create_function(|lua, (id, thread): (AwaitId, Thread)| {
        let env = env(lua);
        let mut scheduler = env.scheduler.borrow_mut();
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
            env(lua)
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
            let env = env(lua);
            let mut scheduler = env.scheduler.borrow_mut();
            scheduler.delayed.insert(id, (func, args));
            scheduler.delay_threads.push((thread.clone(), id));
            Ok(thread)
        })?;
    task.set("delay", delay_fn.clone())?;
    globals.set("delay", delay_fn)?;

    task.set(
        "cancel",
        lua.create_function(|lua, thread: Thread| {
            let env = env(lua);
            let mut scheduler = env.scheduler.borrow_mut();
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
    let time = lua.create_function(|lua, ()| Ok(env(lua).shared.borrow().context.now))?;
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

/// Runs `func(args)` in a new thread now. An error is reported to the
/// Output as the plugin's (`OutputKind::Error`, message plus traceback),
/// never returned.
pub(crate) fn spawn(lua: &Lua, func: Function, args: MultiValue) -> Result<()> {
    start_thread(lua, Value::Function(func), args).map(|_| ())
}

fn report(lua: &Lua, text: String) {
    let env = env(lua);
    env.shared
        .borrow_mut()
        .push_output(&env.name, OutputKind::Error, text);
}

/// Resumes `thread` with `args` and files it by what it yields. Never
/// holds a borrow of `Env::scheduler` while Luau runs.
pub(crate) fn resume(lua: &Lua, thread: Thread, args: MultiValue) -> Result<()> {
    let mut args = args;
    loop {
        if thread.status() != ThreadStatus::Resumable {
            return Ok(());
        }
        let values = match thread.resume::<MultiValue>(args) {
            Ok(values) => values,
            Err(error) => {
                report(lua, error.to_string());
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
        let env = env(lua);
        let mut scheduler = env.scheduler.borrow_mut();
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
            // whoever holds it resumes it, as in Roblox — signal libraries
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

/// A fresh await id (from `Shared::next_id`).
pub(crate) fn new_await(lua: &Lua) -> AwaitId {
    env(lua).shared.borrow_mut().next_id()
}

/// Resumes the thread waiting on `id` (if any) with `(true, values…)` or
/// `(false, message)`.
pub(crate) fn resolve(
    lua: &Lua,
    id: AwaitId,
    result: std::result::Result<MultiValue, String>,
) -> Result<()> {
    let env = env(lua);
    let thread = {
        let mut scheduler = env.scheduler.borrow_mut();
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
    let env = env(lua);
    let now = env.shared.borrow().context.now;
    let seconds = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    env.scheduler
        .borrow_mut()
        .timers
        .push((now + seconds, id, now));
    id
}

/// Resolves due timers, runs `task.delay` functions whose time came, then
/// the deferred queue (only what was queued before this call).
pub(crate) fn tick(lua: &Lua, now: f64) -> Result<()> {
    let env = env(lua);
    env.shared.borrow_mut().context.now = now;
    let (mut due, deferred) = {
        let mut scheduler = env.scheduler.borrow_mut();
        let (due, rest): (Vec<_>, Vec<_>) = scheduler
            .timers
            .drain(..)
            .partition(|(at, _, _)| *at <= now);
        scheduler.timers = rest;
        let deferred: Vec<_> = scheduler.deferred.drain(..).collect();
        (due, deferred)
    };
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, id, started) in due {
        let delayed = {
            let mut scheduler = env.scheduler.borrow_mut();
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
    for (thread, args) in deferred {
        resume(lua, thread, args)?;
    }
    Ok(())
}

/// Whether [`tick`] at `now` would run anything.
pub(crate) fn wants_tick(scheduler: &Scheduler, now: f64) -> bool {
    !scheduler.deferred.is_empty() || scheduler.timers.iter().any(|(due, _, _)| *due <= now)
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::testing::test_vm;

    fn run(lua: &Lua, src: &str) {
        spawn(
            lua,
            lua.load(src).into_function().unwrap(),
            MultiValue::new(),
        )
        .unwrap();
    }

    #[test]
    fn wait_resolves_only_after_time_passes() {
        let (lua, _env, _) = test_vm(WeakDom::new(), false);
        run(
            &lua,
            "done = false local e = task.wait(1) done = true elapsed = e",
        );
        assert!(!lua.globals().get::<bool>("done").unwrap());
        tick(&lua, 0.5).unwrap();
        assert!(!lua.globals().get::<bool>("done").unwrap());
        tick(&lua, 1.25).unwrap();
        assert!(lua.globals().get::<bool>("done").unwrap());
        assert_eq!(lua.globals().get::<f64>("elapsed").unwrap(), 1.25);
    }

    #[test]
    fn delay_runs_function_with_args() {
        let (lua, _env, _) = test_vm(WeakDom::new(), false);
        run(&lua, "task.delay(1, function(x) got = x end, 5)");
        tick(&lua, 0.5).unwrap();
        assert!(lua.globals().get::<Option<f64>>("got").unwrap().is_none());
        tick(&lua, 2.0).unwrap();
        assert_eq!(lua.globals().get::<f64>("got").unwrap(), 5.0);
    }

    #[test]
    fn cancel_stops_delay() {
        let (lua, _env, _) = test_vm(WeakDom::new(), false);
        run(
            &lua,
            "task.cancel(task.delay(1, function() got = true end))",
        );
        tick(&lua, 2.0).unwrap();
        assert!(lua.globals().get::<Option<bool>>("got").unwrap().is_none());
    }

    #[test]
    fn defer_runs_on_next_tick() {
        let (lua, env, _) = test_vm(WeakDom::new(), false);
        run(&lua, "task.defer(function() d = true end)");
        assert!(lua.globals().get::<Option<bool>>("d").unwrap().is_none());
        assert!(wants_tick(&env.scheduler.borrow(), 0.0));
        tick(&lua, 0.0).unwrap();
        assert!(lua.globals().get::<bool>("d").unwrap());
    }

    #[test]
    fn errors_become_output() {
        let (lua, env, _) = test_vm(WeakDom::new(), false);
        run(&lua, "error('boom')");
        let shared = env.shared.borrow();
        assert_eq!(shared.output.len(), 1);
        assert_eq!(shared.output[0].kind, OutputKind::Error);
        assert_eq!(shared.output[0].source, "Test");
        assert!(shared.output[0].text.contains("boom"));
    }

    #[test]
    fn resolve_error_raises_in_thread() {
        let (lua, env, _) = test_vm(WeakDom::new(), false);
        let f = yielding(
            &lua,
            lua.create_function(|lua, ()| Ok(new_await(lua))).unwrap(),
        )
        .unwrap();
        lua.globals().set("op", f).unwrap();
        run(&lua, "local ok, e = pcall(op) result = e");
        let id = *env.scheduler.borrow().waiting.keys().next().unwrap();
        resolve(&lua, id, Err("nope".into())).unwrap();
        assert!(lua
            .globals()
            .get::<String>("result")
            .unwrap()
            .contains("nope"));
    }
}
