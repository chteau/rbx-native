//! What a paused script exposes: its call stack, the variables in scope and
//! expressions evaluated against them.

use std::cell::RefCell;
use std::ffi::{c_int, CStr};
use std::mem;

use mlua::{ffi, Lua, MultiValue, Table, Value};

/// One row of the Call Stack: a Luau function and the line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub function: String,
    pub line: u32,
}

/// One variable in scope, already formatted for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
}

/// The script as it stands at a pause. Only valid inside `on_pause`: it
/// reads the frozen thread's stack directly.
pub struct Paused<'a> {
    lua: &'a Lua,
    state: *mut ffi::lua_State,
    line: u32,
    /// `lua_stackdepth` at the pause. Stack levels are counted from the
    /// innermost call, and reading the scope goes through mlua's own
    /// protected call, which may add a frame on this same thread; the depth
    /// is what finds the paused function again from under it.
    depth: c_int,
    output: &'a RefCell<Vec<String>>,
}

/// How deep a table is spelled out before it is shown as `{...}`.
const TABLE_DEPTH: usize = 2;
/// How many entries of one table are spelled out.
const TABLE_ENTRIES: usize = 8;

impl<'a> Paused<'a> {
    pub(super) fn new(
        lua: &'a Lua,
        state: *mut ffi::lua_State,
        line: u32,
        output: &'a RefCell<Vec<String>>,
    ) -> Self {
        // SAFETY: only reads the thread's call-info count.
        let depth = unsafe { ffi::lua_stackdepth(state) };
        Paused {
            lua,
            state,
            line,
            depth,
            output,
        }
    }

    /// The line about to run, 1-based.
    pub fn line(&self) -> u32 {
        self.line
    }

    /// Everything printed since the run started or this was last called, so
    /// a paused script's output can be shown before the run is over.
    pub fn take_output(&self) -> Vec<String> {
        std::mem::take(&mut self.output.borrow_mut())
    }

    /// Innermost first; native functions (Rust bindings, `pcall`) are left
    /// out, as Studio's Call Stack leaves out the engine's own frames.
    pub fn call_stack(&self) -> Vec<Frame> {
        let mut frames = Vec::new();
        for level in 0.. {
            // SAFETY: `lua_getinfo` only writes `ar`; levels past the stack
            // return 0, which ends the walk.
            unsafe {
                let mut ar = mem::zeroed::<ffi::lua_Debug>();
                if ffi::lua_getinfo(self.state, level, c"sln".as_ptr(), &mut ar) == 0 {
                    break;
                }
                if ar.currentline < 0 {
                    continue;
                }
                let function = match c_str(ar.name) {
                    Some(name) if !name.is_empty() => name,
                    _ => format!("function <line {}>", ar.linedefined),
                };
                frames.push(Frame {
                    function,
                    line: ar.currentline as u32,
                });
            }
        }
        // The chunk is always called from Rust, so the outermost Luau frame
        // is its main function; Luau gives it no name of its own.
        if let Some(outermost) = frames.last_mut() {
            outermost.function = "main chunk".to_owned();
        }
        frames
    }

    /// The innermost function's locals and upvalues, as Studio's Watch
    /// window lists them under Variables.
    pub fn variables(&self) -> Vec<Variable> {
        self.scope()
            .into_iter()
            .map(|(name, value)| Variable {
                value: describe(&value, TABLE_DEPTH),
                name,
            })
            .collect()
    }

    /// Evaluates `expression` in the innermost function's scope; several
    /// results are joined with `, `.
    pub fn evaluate(&self, expression: &str) -> Result<String, String> {
        let values = self.eval(expression)?;
        Ok(values
            .iter()
            .map(|value| describe(value, TABLE_DEPTH))
            .collect::<Vec<_>>()
            .join(", "))
    }

    pub(super) fn truthy(&self, expression: &str) -> Result<bool, String> {
        let values = self.eval(expression)?;
        Ok(!matches!(
            values.front(),
            None | Some(Value::Nil) | Some(Value::Boolean(false))
        ))
    }

    /// What `print(<expression list>)` would print.
    pub(super) fn print_line(&self, expressions: &str) -> Result<String, String> {
        let values = self.eval(expressions)?;
        let parts: Result<Vec<String>, _> = values.iter().map(Value::to_string).collect();
        Ok(parts.map_err(|err| err.to_string())?.join(" "))
    }

    fn eval(&self, expression: &str) -> Result<MultiValue, String> {
        let env = self.environment().map_err(|err| err.to_string())?;
        self.lua
            .load(format!("return {expression}"))
            .set_name("=watch")
            .set_environment(env)
            .eval::<MultiValue>()
            .map_err(|err| one_line(&err))
    }

    /// A table of the scope, falling back to the globals for everything else.
    fn environment(&self) -> mlua::Result<Table> {
        let env = self.lua.create_table()?;
        for (name, value) in self.scope() {
            env.raw_set(name, value)?;
        }
        let meta = self.lua.create_table()?;
        meta.raw_set("__index", self.lua.globals())?;
        env.set_metatable(Some(meta))?;
        Ok(env)
    }

    /// Upvalues then locals, so a local shadows an upvalue of the same name
    /// and, among locals, the innermost block's wins — the order `lua_getlocal`
    /// numbers them in.
    fn scope(&self) -> Vec<(String, Value)> {
        let mut names = Vec::new();
        let paused = self.state;
        let depth = self.depth;
        // SAFETY: values are pushed on the paused thread by the debug API and
        // moved to mlua's current state, where `exec_raw` collects them as its
        // results; `lua_xmove` is a no-op when both are the same thread.
        let values = unsafe {
            self.lua.exec_raw::<MultiValue>((), |state| {
                let level = ffi::lua_stackdepth(paused) - depth;
                let mut ar = mem::zeroed::<ffi::lua_Debug>();
                if ffi::lua_getinfo(paused, level, c"f".as_ptr(), &mut ar) != 0 {
                    // An absolute index: when both are one thread the
                    // upvalues land above the function, not in its place.
                    let function = ffi::lua_gettop(paused);
                    for n in 1.. {
                        ffi::lua_checkstack(state, 2);
                        let name = ffi::lua_getupvalue(paused, function, n);
                        if name.is_null() {
                            break;
                        }
                        keep(paused, state, name, &mut names);
                    }
                    ffi::lua_remove(paused, function);
                }
                for n in 1.. {
                    ffi::lua_checkstack(state, 2);
                    let name = ffi::lua_getlocal(paused, level, n);
                    if name.is_null() {
                        break;
                    }
                    keep(paused, state, name, &mut names);
                }
            })
        };
        let Ok(values) = values else {
            return Vec::new();
        };
        let mut scope: Vec<(String, Value)> = Vec::new();
        for (name, value) in names.into_iter().zip(values) {
            match scope.iter_mut().find(|(existing, _)| *existing == name) {
                Some(slot) => slot.1 = value,
                None => scope.push((name, value)),
            }
        }
        scope
    }
}

/// Moves the value just pushed on `paused` over to `state`, or drops it if
/// it has no user-facing name (Luau's `(for state)`-style internals, or an
/// upvalue compiled without debug info).
unsafe fn keep(
    paused: *mut ffi::lua_State,
    state: *mut ffi::lua_State,
    name: *const std::ffi::c_char,
    names: &mut Vec<String>,
) {
    match c_str(name) {
        Some(name) if !name.is_empty() && !name.starts_with('(') => {
            ffi::lua_xmove(paused, state, 1);
            names.push(name);
        }
        _ => ffi::lua_pop(paused, 1),
    }
}

/// An evaluation error as one line: the message without the `runtime
/// error: ` prefix or the traceback mlua appends, which only ever points at
/// the watch's own one-line chunk.
fn one_line(err: &mlua::Error) -> String {
    let text = err.to_string();
    let first = text.lines().next().unwrap_or_default();
    first
        .strip_prefix("runtime error: ")
        .unwrap_or(first)
        .to_owned()
}

unsafe fn c_str(ptr: *const std::ffi::c_char) -> Option<String> {
    (!ptr.is_null()).then(|| CStr::from_ptr(ptr).to_string_lossy().into_owned())
}

/// A value as the Watch window shows it: strings quoted, tables spelled out
/// a couple of levels deep, everything else as `tostring` has it.
pub(crate) fn describe(value: &Value, depth: usize) -> String {
    match value {
        Value::String(text) => format!("{:?}", text.to_string_lossy()),
        Value::Table(table) if depth == 0 => {
            if table.raw_len() == 0 && table.pairs::<Value, Value>().next().is_none() {
                "{}".to_owned()
            } else {
                "{...}".to_owned()
            }
        }
        Value::Table(table) => {
            let mut entries = Vec::new();
            let mut more = false;
            for pair in table.pairs::<Value, Value>() {
                let Ok((key, value)) = pair else { continue };
                if entries.len() == TABLE_ENTRIES {
                    more = true;
                    break;
                }
                let key = match &key {
                    Value::String(text) => text.to_string_lossy(),
                    other => format!("[{}]", describe(other, 0)),
                };
                entries.push(format!("{key} = {}", describe(&value, depth - 1)));
            }
            if more {
                entries.push("...".to_owned());
            }
            format!("{{{}}}", entries.join(", "))
        }
        other => other
            .to_string()
            .unwrap_or_else(|_| other.type_name().to_owned()),
    }
}
