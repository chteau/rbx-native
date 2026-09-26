//! Line breakpoints and stepping over Luau's own single-step hook.
//!
//! Luau has no `debug.sethook`; what it has instead is `lua_singlestep`, which
//! makes the VM call the global `debugstep` callback before every instruction.
//! That hook is the whole debugger: it notices when execution reaches a new
//! line, decides whether a breakpoint or a step request wants it stopped
//! there, and if so calls the caller's `on_pause` — synchronously, on the
//! thread running the script, which is what "paused" means here. Whatever
//! `on_pause` does before returning (block on a channel, evaluate watches,
//! walk the call stack) happens with the script frozen mid-line.

mod frame;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_int;
use std::rc::Rc;

use mlua::chunk::Compiler;
use mlua::{ffi, Lua};

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

#[derive(Debug, Clone, Copy)]
enum Step {
    None,
    Into,
    Over(c_int),
    Out(c_int),
}

struct Session {
    breakpoints: HashMap<u32, Breakpoint>,
    on_pause: Box<dyn FnMut(&Paused) -> Resume>,
    step: Step,
    /// The line last reported at each call depth. A line only counts as
    /// reached when it differs from its own depth's entry, so returning from a
    /// call to the rest of the calling line is not "a new line", and a loop
    /// jumping back to its header is.
    lines: Vec<c_int>,
    output: Rc<RefCell<Vec<String>>>,
}

/// Held in the VM's app data for as long as a debug run lasts. `busy` is
/// what keeps the hook from re-entering itself: evaluating a condition or a
/// watch runs Luau on the same, still single-stepping, thread.
struct Hook {
    busy: Cell<bool>,
    session: RefCell<Session>,
}

/// Runs `source` with the debugger attached, compiled at full debug info and
/// no optimization so every line and every local survives to be inspected.
pub(crate) fn run(
    lua: &Lua,
    source: &str,
    name: &str,
    breakpoints: &[Breakpoint],
    output: Rc<RefCell<Vec<String>>>,
    on_pause: impl FnMut(&Paused) -> Resume + 'static,
) -> mlua::Result<()> {
    let chunk = lua
        .load(source)
        .set_name(format!("={name}"))
        .set_compiler(debug_compiler())
        .into_function()?;

    lua.set_app_data(Rc::new(Hook {
        busy: Cell::new(false),
        session: RefCell::new(Session {
            breakpoints: breakpoints.iter().map(|b| (b.line, b.clone())).collect(),
            on_pause: Box::new(on_pause),
            step: Step::None,
            lines: Vec::new(),
            output,
        }),
    }));
    // SAFETY: `lua_callbacks` returns the VM's global callback table, which
    // lives as long as the VM; the flag is set on the very state `call`
    // below runs the chunk on (both use mlua's current state).
    unsafe {
        lua.exec_raw::<()>((), |state| {
            (*ffi::lua_callbacks(state)).debugstep = Some(debug_step);
            ffi::lua_singlestep(state, 1);
        })?;
    }

    let result = chunk.call::<()>(());

    // SAFETY: as above.
    unsafe {
        lua.exec_raw::<()>((), |state| {
            ffi::lua_singlestep(state, 0);
            (*ffi::lua_callbacks(state)).debugstep = None;
        })?;
    }
    lua.remove_app_data::<Rc<Hook>>();
    result
}

fn debug_compiler() -> Compiler {
    Compiler::new()
        .set_optimization_level(0)
        .set_debug_level(2)
        .set_mutable_globals(["game", "workspace", "Workspace"])
}

unsafe extern "C-unwind" fn debug_step(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug) {
    // Scoped so every Rust value is dropped before `lua_error` unwinds past
    // this frame.
    let stop = {
        let lua = Lua::get_or_init_from_ptr(state);
        let Some(hook) = lua.app_data_ref::<Rc<Hook>>().map(|hook| Rc::clone(&hook)) else {
            return;
        };
        if hook.busy.replace(true) {
            return;
        }
        let resume = step(lua, &hook, state, (*ar).currentline);
        hook.busy.set(false);
        resume == Some(Resume::Stop)
    };
    if stop {
        ffi::lua_pushlstring_(state, STOPPED.as_ptr().cast(), STOPPED.len());
        ffi::lua_error(state);
    }
}

/// One instruction's worth of the debugger: `None` unless the script paused.
unsafe fn step(lua: &Lua, hook: &Hook, state: *mut ffi::lua_State, line: c_int) -> Option<Resume> {
    if line < 0 {
        return None;
    }
    let depth = ffi::lua_stackdepth(state);
    let mut session = hook.session.borrow_mut();
    let slot = depth.max(0) as usize;
    session.lines.truncate(slot + 1);
    session.lines.resize(slot + 1, -1);
    if session.lines[slot] == line {
        return None;
    }
    session.lines[slot] = line;

    let stepped = match session.step {
        Step::None => false,
        Step::Into => true,
        Step::Over(from) => depth <= from,
        Step::Out(from) => depth < from,
    };
    let paused = Paused::new(lua, state, line as u32);
    let breaks = match session.breakpoints.get(&(line as u32)) {
        Some(breakpoint) => activate(&paused, breakpoint, &session.output),
        None => false,
    };
    if !stepped && !breaks {
        return None;
    }

    let resume = (session.on_pause)(&paused);
    session.step = match resume {
        Resume::Continue | Resume::Stop => Step::None,
        Resume::StepInto => Step::Into,
        Resume::StepOver => Step::Over(depth),
        Resume::StepOut => Step::Out(depth),
    };
    Some(resume)
}

/// Whether `breakpoint` pauses here, logging its message on the way.
fn activate(paused: &Paused, breakpoint: &Breakpoint, output: &RefCell<Vec<String>>) -> bool {
    if let Some(condition) = &breakpoint.condition {
        match paused.truthy(condition) {
            Ok(false) => return false,
            Ok(true) => {}
            Err(err) => output
                .borrow_mut()
                .push(format!("Breakpoint condition `{condition}` failed: {err}")),
        }
    }
    if let Some(message) = &breakpoint.log_message {
        let line = paused
            .print_line(message)
            .unwrap_or_else(|err| format!("Logpoint `{message}` failed: {err}"));
        output.borrow_mut().push(line);
    }
    !breakpoint.continue_execution
}
