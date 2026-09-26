//! The VM callbacks and the state they share.
//!
//! One handler serves both instruction callbacks: `debugbreak`, for an
//! instruction patched into `LOP_BREAK` on a breakpoint line, and
//! `debugstep`, for every other instruction while that callback is
//! installed. It decides two things — whether a breakpoint line is being
//! passed through anew, and whether a step request is done — and pauses if
//! either says so.
//!
//! **A pass through a breakpoint line.** Luau's `LOP_BREAK` fires on every
//! instruction of the line, and the VM exposes no program counter to tell
//! the first of a pass from the rest. So the first `LOP_BREAK` counts, and
//! the line is then *watched*: its patches are lifted and `debugstep` takes
//! over until execution leaves it. While watched, a new pass is a new
//! statement starting on the line — the chunk is compiled with statement
//! coverage, whose per-line counter (`lua_getcoverage`, the busiest of the
//! line's statements) goes up once each time. That is what makes each
//! iteration of `for i = 1, 3 do f() end` a pass of its own. The counter's
//! first rise after entering the line belongs to that same entry, so it is
//! absorbed.
//!
//! **Where each frame is.** While `debugstep` is installed, the line last
//! seen at each call depth is kept; a line is "reached" when its depth's
//! entry changes. Returning from a call to the rest of the calling line
//! therefore does not count, and a loop jumping back to its header does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::mem;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mlua::{ffi, Function, Lua};

use super::frame::Compiled;
use super::{Breakpoint, Paused, Resume, STOPPED};

/// Where the chunk being debugged is kept, so a callback can patch its
/// lines. mlua's named registry values are plain string keys.
const CHUNK_KEY: &CStr = c"rbx_lua.debugger.chunk";

thread_local! {
    /// The running session's stop flag, for the interrupt callback: it runs
    /// at every call and loop back-edge, too often to go through mlua.
    static STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy)]
enum Step {
    None,
    Into,
    Over(c_int),
    Out(c_int),
}

/// A breakpoint line execution is on, with its `LOP_BREAK`s lifted.
struct Watch {
    line: c_int,
    /// The shallowest call depth on the line; leaving is judged from here.
    depth: c_int,
    /// The line's coverage counter when last looked at.
    hits: c_int,
    /// Whether the counter's next rise is the current pass's own.
    absorb: bool,
}

struct Session {
    /// By the line the VM actually stops on (see [`attach`]).
    breakpoints: HashMap<c_int, Breakpoint>,
    on_pause: Box<dyn FnMut(&Paused) -> Resume>,
    step: Step,
    /// Whether `debugstep` is installed.
    tracking: bool,
    /// The line last seen at each call depth; only kept while tracking.
    lines: Vec<c_int>,
    watches: Vec<Watch>,
    output: Rc<RefCell<Vec<String>>>,
    compiled: Rc<Compiled>,
}

/// Held in the VM's app data for as long as a debug run lasts. `busy` is
/// what keeps the handler from re-entering itself: evaluating a condition or
/// a watch runs Luau on the same thread.
struct Hook {
    busy: Cell<bool>,
    stop: Arc<AtomicBool>,
    session: RefCell<Session>,
}

/// The attached debugger; dropping it detaches it, however the run ended.
pub(super) struct Attached<'a> {
    lua: &'a Lua,
}

/// Patches the breakpoints into `chunk` and installs the callbacks.
pub(super) fn attach<'a>(
    lua: &'a Lua,
    chunk: &Function,
    breakpoints: &[Breakpoint],
    output: Rc<RefCell<Vec<String>>>,
    stop: Arc<AtomicBool>,
    on_pause: Box<dyn FnMut(&Paused) -> Resume>,
) -> mlua::Result<Attached<'a>> {
    lua.set_named_registry_value(key(), chunk.clone())?;
    // By the line each one really lands on: `lua_breakpoint` moves one set
    // on a blank or comment line to the next line with code.
    let mut placed = HashMap::new();
    // SAFETY: the chunk is the argument `exec_raw` pushed, at the top of the
    // stack; the callback table is the VM's own and outlives this run. The
    // single-step flag goes on mlua's current state, the one `Function::call`
    // runs the chunk on.
    unsafe {
        lua.exec_raw::<()>(chunk.clone(), |state| {
            for breakpoint in breakpoints {
                let line = ffi::lua_breakpoint(state, -1, breakpoint.line as c_int, 1);
                if line >= 0 {
                    placed.entry(line).or_insert_with(|| breakpoint.clone());
                }
            }
            let callbacks = ffi::lua_callbacks(state);
            (*callbacks).debugbreak = Some(debug_break);
            (*callbacks).interrupt = Some(interrupt);
            ffi::lua_singlestep(state, 1);
        })?;
    }
    STOP.with(|slot| *slot.borrow_mut() = Some(stop.clone()));
    lua.set_app_data(Rc::new(Hook {
        busy: Cell::new(false),
        stop,
        session: RefCell::new(Session {
            breakpoints: placed,
            on_pause,
            step: Step::None,
            tracking: false,
            lines: Vec::new(),
            watches: Vec::new(),
            output,
            compiled: Rc::default(),
        }),
    }));
    Ok(Attached { lua })
}

impl Drop for Attached<'_> {
    fn drop(&mut self) {
        // SAFETY: as in `attach`.
        let _ = unsafe {
            self.lua.exec_raw::<()>((), |state| {
                ffi::lua_singlestep(state, 0);
                let callbacks = ffi::lua_callbacks(state);
                (*callbacks).debugstep = None;
                (*callbacks).debugbreak = None;
                (*callbacks).interrupt = None;
            })
        };
        STOP.with(|slot| slot.borrow_mut().take());
        self.lua.remove_app_data::<Rc<Hook>>();
        let _ = self.lua.unset_named_registry_value(key());
    }
}

fn key() -> &'static str {
    CHUNK_KEY.to_str().expect("an ASCII key")
}

unsafe extern "C-unwind" fn interrupt(state: *mut ffi::lua_State, gc: c_int) {
    // A GC step is no place to raise an error.
    if gc >= 0 {
        return;
    }
    let stop = STOP.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    });
    if stop {
        raise_stop(state);
    }
}

unsafe extern "C-unwind" fn debug_step(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug) {
    dispatch(state, (*ar).currentline);
}

unsafe extern "C-unwind" fn debug_break(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug) {
    dispatch(state, (*ar).currentline);
}

unsafe fn dispatch(state: *mut ffi::lua_State, line: c_int) {
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
        let resume = match hook.stop.load(Ordering::Relaxed) {
            true => Some(Resume::Stop),
            false => hook.session.borrow_mut().on_line(lua, state, line),
        };
        hook.busy.set(false);
        let stop = resume == Some(Resume::Stop);
        // Latched, so a `pcall` in the script that swallows the error below
        // is stopped again at its next call, return or loop.
        if stop {
            hook.stop.store(true, Ordering::Relaxed);
        }
        stop
    };
    if stop {
        raise_stop(state);
    }
}

unsafe fn raise_stop(state: *mut ffi::lua_State) -> ! {
    ffi::lua_pushlstring_(state, STOPPED.as_ptr().cast(), STOPPED.len());
    ffi::lua_error(state)
}

impl Session {
    /// One instruction's worth of the debugger: `None` unless the script
    /// paused.
    unsafe fn on_line(
        &mut self,
        lua: &Lua,
        state: *mut ffi::lua_State,
        line: c_int,
    ) -> Option<Resume> {
        if line < 0 {
            return None;
        }
        let depth = ffi::lua_stackdepth(state);
        // Untracked, only a `LOP_BREAK` gets here, and only on the first
        // instruction of a pass (see `new_pass`).
        let reached = !self.tracking || self.reached(depth, line);
        self.leave_watches(state, depth, line);

        let output = Rc::clone(&self.output);
        let compiled = Rc::clone(&self.compiled);
        let mut breaks = false;
        if let Some(breakpoint) = self.breakpoints.get(&line).cloned() {
            if self.new_pass(state, depth, line, reached) {
                let paused = Paused::new(lua, state, line as u32, &output, &compiled);
                breaks = activate(&paused, &breakpoint, &output);
            }
        }
        let stepped = reached
            && match self.step {
                Step::None => false,
                Step::Into => true,
                Step::Over(from) => depth <= from,
                Step::Out(from) => depth < from,
            };
        if !stepped && !breaks {
            self.untrack_if_idle(state);
            return None;
        }

        let resume = (self.on_pause)(&Paused::new(lua, state, line as u32, &output, &compiled));
        self.step = match resume {
            Resume::Continue | Resume::Stop => Step::None,
            Resume::StepInto => Step::Into,
            Resume::StepOver => Step::Over(depth),
            Resume::StepOut => Step::Out(depth),
        };
        match self.step {
            Step::None => self.untrack_if_idle(state),
            _ => self.track(state),
        }
        Some(resume)
    }

    /// Whether this instruction starts a pass through breakpoint `line`.
    unsafe fn new_pass(
        &mut self,
        state: *mut ffi::lua_State,
        depth: c_int,
        line: c_int,
        reached: bool,
    ) -> bool {
        match self.watches.iter_mut().find(|watch| watch.line == line) {
            None => {
                patch(state, line, false);
                self.watches.push(Watch {
                    line,
                    depth,
                    hits: coverage(state, line),
                    absorb: true,
                });
                self.track(state);
                true
            }
            Some(watch) if reached => {
                watch.depth = watch.depth.min(depth);
                watch.hits = coverage(state, line);
                watch.absorb = true;
                true
            }
            Some(watch) => {
                let hits = coverage(state, line);
                if hits <= watch.hits {
                    return false;
                }
                watch.hits = hits;
                !mem::replace(&mut watch.absorb, false)
            }
        }
    }

    /// Ends every watch execution has left — back at or above the depth that
    /// entered it, on another line — putting its `LOP_BREAK`s back.
    unsafe fn leave_watches(&mut self, state: *mut ffi::lua_State, depth: c_int, line: c_int) {
        self.watches.retain(|watch| {
            let left = depth <= watch.depth && line != watch.line;
            if left {
                patch(state, watch.line, true);
            }
            !left
        });
    }

    fn reached(&mut self, depth: c_int, line: c_int) -> bool {
        let slot = depth.max(0) as usize;
        self.lines.truncate(slot + 1);
        self.lines.resize(slot + 1, -1);
        mem::replace(&mut self.lines[slot], line) != line
    }

    unsafe fn track(&mut self, state: *mut ffi::lua_State) {
        if self.tracking {
            return;
        }
        (*ffi::lua_callbacks(state)).debugstep = Some(debug_step);
        self.tracking = true;
        // Every frame's current line, so nothing already on screen counts as
        // newly reached.
        let depth = ffi::lua_stackdepth(state);
        self.lines = vec![-1; depth.max(0) as usize + 1];
        for level in 0..=depth {
            let mut ar = mem::zeroed::<ffi::lua_Debug>();
            if ffi::lua_getinfo(state, level, c"l".as_ptr(), &mut ar) == 0 {
                break;
            }
            self.lines[(depth - level) as usize] = ar.currentline;
        }
    }

    unsafe fn untrack_if_idle(&mut self, state: *mut ffi::lua_State) {
        if self.tracking && matches!(self.step, Step::None) && self.watches.is_empty() {
            (*ffi::lua_callbacks(state)).debugstep = None;
            self.tracking = false;
        }
    }
}

/// Sets or lifts the `LOP_BREAK`s on one line of the chunk.
unsafe fn patch(state: *mut ffi::lua_State, line: c_int, enabled: bool) {
    ffi::lua_rawgetfield(state, ffi::LUA_REGISTRYINDEX, CHUNK_KEY.as_ptr());
    if ffi::lua_isfunction(state, -1) != 0 {
        ffi::lua_breakpoint(state, -1, line, enabled as c_int);
    }
    ffi::lua_pop(state, 1);
}

/// The running function's coverage counter for `line`: how many times its
/// busiest statement there has started. -1 for a line with none.
unsafe fn coverage(state: *mut ffi::lua_State, line: c_int) -> c_int {
    struct Probe {
        line: usize,
        hits: c_int,
    }
    unsafe extern "C-unwind" fn visit(
        context: *mut c_void,
        _function: *const c_char,
        _linedefined: c_int,
        depth: c_int,
        hits: *const c_int,
        size: usize,
    ) {
        let probe = &mut *context.cast::<Probe>();
        // Depth 0 is the function itself; the rest are functions nested in it.
        if depth == 0 && probe.line < size {
            probe.hits = *hits.add(probe.line);
        }
    }

    let mut ar = mem::zeroed::<ffi::lua_Debug>();
    if ffi::lua_getinfo(state, 0, c"f".as_ptr(), &mut ar) == 0 {
        return -1;
    }
    let mut probe = Probe {
        line: line.max(0) as usize,
        hits: -1,
    };
    ffi::lua_getcoverage(state, -1, (&mut probe as *mut Probe).cast(), visit);
    ffi::lua_pop(state, 1);
    probe.hits
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
