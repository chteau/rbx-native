//! Synchronous Luau scripting over a `WeakDom`, in the spirit of Studio's
//! command bar.
//!
//! [`Runtime`] owns the DOM while a script runs and exposes it to Luau through
//! `game`, `workspace`, `Instance`, `Enum` and a first batch of datatypes. There
//! is no scheduler: scripts run to completion, with no events and no yielding.

mod ctx;
mod datatypes;
mod debugger;
mod defaults;
mod game;
mod hooks;
mod instance;
mod lua_enum;
mod not_creatable;
pub mod pivot;
pub mod plugin;
mod printed;
mod property;
mod runtime;

pub use debugger::{Breakpoint, Frame, Paused, Resume, Variable, STOPPED};
pub use mlua::Error as LuaError;
pub use printed::{PrintPart, PrintValue, TableDump, TableEntry};
pub use runtime::{Output, Runtime};

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
