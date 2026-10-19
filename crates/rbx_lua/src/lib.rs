//! Luau scripting over a `WeakDom`, in the spirit of Studio's command bar.
//!
//! [`Runtime`] holds the DOM while a script runs and exposes it to Luau through
//! `game`, `workspace`, `Instance`, `Enum` and the datatypes. Scripts can
//! yield (`task.wait`, `Signal:Wait`): the host resumes the run until it is
//! done, lending the DOM for each resume. The plugin host
//! ([`plugin::PluginHost`]) shares the same scheduler, signals and events.

mod compat;
mod ctx;
mod datatypes;
mod debugger;
mod defaults;
mod engine;
mod game;
mod hooks;
mod instance;
mod lua_enum;
mod not_creatable;
mod output;
pub mod pivot;
pub mod plugin;
mod printed;
mod property;
mod runtime;

pub use debugger::{Breakpoint, Frame, Paused, Resume, Variable, STOPPED};
pub use mlua::Error as LuaError;
pub use output::{LineKind, Output};
pub use printed::{PrintPart, PrintValue, TableDump, TableEntry};
pub use runtime::{RunError, RunState, Runtime, Step};

/// Whether `source` compiles as Luau (nothing runs). `Err` is the
/// compiler's message, e.g. a syntax error or "too many upvalues".
pub fn compile_check(source: &str, name: &str) -> Result<(), String> {
    mlua::Lua::new()
        .load(source)
        .set_name(name)
        .into_function()
        .map(drop)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod compile_check_tests {
    #[test]
    fn compile_check_reports_syntax_errors() {
        assert!(super::compile_check("local a = 1 return a", "ok").is_ok());
        assert!(super::compile_check("local = ", "bad").is_err());
    }
}
