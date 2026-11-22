//! What a VM's scripts cost: `debug.profilebegin`/`profileend` scopes for
//! the MicroProfiler-style timeline, the memory categories of
//! `debug.setmemorycategory`, the time spent in Luau per frame, and the
//! sampling ScriptProfiler behind `ScriptProfilerService` (its JSON is the
//! Developer Console's export format, version 2).
//!
//! The sampler hangs off Luau's interrupt, which fires at every call and
//! loop back edge: mlua's own interrupt (the script timeout) is wrapped by
//! `sampler::sample_proc`, which reads the running coroutine's stack (mlua's
//! callback only sees the main thread) and then hands over to it.

mod sampler;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Lua, Value, Variadic};

pub(crate) use sampler::arm;
use sampler::Sampler;

/// One `debug.profilebegin` … `profileend` scope that closed.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileLabel {
    pub name: String,
    pub start: Instant,
    pub end: Instant,
    /// How many scopes were open around it.
    pub depth: u16,
}

/// Roblox's sampling rates: the Developer Console offers 1 KHz (default)
/// and 10 KHz.
pub const DEFAULT_FREQUENCY: u32 = 1000;

/// A VM's profiling state, kept in its app data.
#[derive(Default)]
pub struct Profiler {
    open: Vec<(String, Instant)>,
    labels: Vec<ProfileLabel>,
    category: Option<String>,
    memory_category: Option<String>,
    script_time: Duration,
    sampler: Option<Sampler>,
    /// The last finished session's JSON, for `RequestData` after a stop.
    last: Option<String>,
    /// A running game's VM (a server or client of a run), not edit mode.
    pub(crate) live: bool,
}

#[derive(Clone)]
struct Shared(Rc<RefCell<Profiler>>);

/// The profiler of `lua`, created on first use.
pub(crate) fn of(lua: &Lua) -> Rc<RefCell<Profiler>> {
    if let Some(shared) = lua.app_data_ref::<Shared>() {
        return shared.0.clone();
    }
    let shared = Rc::new(RefCell::new(Profiler::default()));
    lua.set_app_data(Shared(shared.clone()));
    shared
}

impl Profiler {
    /// The scopes closed since the last call, in the order they closed.
    pub fn take_labels(&mut self) -> Vec<ProfileLabel> {
        std::mem::take(&mut self.labels)
    }

    /// Wall time spent running Luau since the last call.
    pub fn take_script_time(&mut self) -> Duration {
        std::mem::take(&mut self.script_time)
    }

    pub(crate) fn add_script_time(&mut self, spent: Duration) {
        self.script_time += spent;
    }

    /// What the threads about to run are, for the sampler's callgraph:
    /// `Heartbeat`, `Stepped`, `Wait`…
    pub(crate) fn set_category(&mut self, name: &str) {
        if self.category.as_deref() != Some(name) {
            self.category = Some(name.to_owned());
        }
    }

    /// The category `debug.setmemorycategory` last set, if any.
    pub fn memory_category(&self) -> Option<&str> {
        self.memory_category.as_deref()
    }

    pub fn profiling(&self) -> bool {
        self.sampler.is_some()
    }

    /// Starts a ScriptProfiler session sampling `frequency` times a
    /// second; a running one starts over.
    pub fn start(&mut self, frequency: u32) {
        self.sampler = Some(Sampler::new(frequency));
    }

    /// Ends the session, keeping its JSON for [`Profiler::data`].
    pub fn stop(&mut self) {
        if let Some(sampler) = self.sampler.take() {
            self.last = Some(sampler.json());
        }
    }

    /// The running session's data so far, else the last one's, else an
    /// empty session: the JSON `OnNewData` carries.
    pub fn data(&self) -> String {
        match (&self.sampler, &self.last) {
            (Some(sampler), _) => sampler.json(),
            (None, Some(last)) => last.clone(),
            (None, None) => Sampler::new(DEFAULT_FREQUENCY).json(),
        }
    }

    /// A fresh resumption starts now: time before it was idle, not script.
    pub(crate) fn resumed(&mut self) {
        if let Some(sampler) = &mut self.sampler {
            sampler.last = Instant::now();
        }
    }
}

/// `debug.profilebegin(label)` / `debug.profileend()` and
/// `debug.setmemorycategory(tag)` / `resetmemorycategory()`.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let debug: mlua::Table = lua.globals().get("debug")?;
    debug.set(
        "profilebegin",
        lua.create_function(|lua, label: Variadic<Value>| {
            // Measured on a live server (ProfilerApi golden): no label at
            // all has words of its own; a number is taken as text.
            if matches!(label.first(), None | Some(Value::Nil)) {
                return Err(mlua::Error::runtime(
                    "debug.profilebegin(string) parameter must be string, got an unexpected \
                     type. Not opening profile block.",
                ));
            }
            let name = string_arg(&label, "profilebegin")?;
            of(lua).borrow_mut().open.push((name, Instant::now()));
            Ok(())
        })?,
    )?;
    debug.set(
        "profileend",
        lua.create_function(|lua, _: Variadic<Value>| {
            let profiler = of(lua);
            let mut profiler = profiler.borrow_mut();
            if let Some((name, start)) = profiler.open.pop() {
                let depth = profiler.open.len() as u16;
                profiler.labels.push(ProfileLabel {
                    name,
                    start,
                    end: Instant::now(),
                    depth,
                });
            }
            Ok(())
        })?,
    )?;
    debug.set(
        "setmemorycategory",
        lua.create_function(|lua, tag: Variadic<Value>| {
            if matches!(tag.first(), None | Some(Value::Nil)) {
                return Err(mlua::Error::runtime(
                    "missing argument #1 to 'setmemorycategory' (string expected)",
                ));
            }
            let name = string_arg(&tag, "setmemorycategory")?;
            let profiler = of(lua);
            let mut profiler = profiler.borrow_mut();
            let previous = profiler.memory_category.replace(name);
            drop(profiler);
            // Before any is set, the category is the running script's
            // name (ProfilerApi: the root `Script`, not a module it
            // required).
            Ok(previous.unwrap_or_else(|| {
                crate::engine::scheduler::script_frames(lua)
                    .last()
                    .and_then(|(script, _, _)| script.rsplit('.').next().map(str::to_owned))
                    .unwrap_or_default()
            }))
        })?,
    )?;
    debug.set(
        "resetmemorycategory",
        lua.create_function(|lua, _: Variadic<Value>| {
            of(lua).borrow_mut().memory_category = None;
            Ok(())
        })?,
    )?;
    Ok(())
}

fn string_arg(args: &[Value], function: &str) -> mlua::Result<String> {
    match args.first() {
        Some(Value::String(s)) => Ok(s.to_string_lossy().to_string()),
        Some(Value::Integer(n)) => Ok(n.to_string()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        other => Err(mlua::Error::runtime(format!(
            "invalid argument #1 to '{function}' (string expected, got {})",
            other.map_or("no value", Value::type_name)
        ))),
    }
}
