//! Luau scripting over a `WeakDom`, in the spirit of Studio's command bar.
//!
//! [`Runtime`] holds the DOM while a script runs and exposes it to Luau through
//! `game`, `workspace`, `Instance`, `Enum` and the datatypes. Scripts can
//! yield (`task.wait`, `Signal:Wait`): the host resumes the run until it is
//! done, lending the DOM for each resume. The plugin host
//! ([`plugin::PluginHost`]) shares the same scheduler, signals and events.

mod compat;
pub mod csg;
mod ctx;
mod datatypes;
mod debugger;
mod defaults;
mod engine;
mod game;
mod hooks;
mod instance;
pub mod leaks;
mod lua_enum;
mod luau;
mod not_creatable;
mod output;
mod particles;
pub mod pattern;
pub mod pivot;
pub mod plugin;
mod printed;
pub mod profiler;
mod property;
pub use property::clock;
mod runtime;
pub mod server;
mod studio;
pub mod tape;
pub mod test_runner;

pub use debugger::{Breakpoint, Frame, Paused, Resume, Variable, STOPPED};
pub use luau::{
    linked_version as luau_linked_version, set_if_local, IF_LOCAL_FFLAGS, LUAU_VERSION,
};
pub use mlua::Error as LuaError;
pub use output::{LineKind, Output};
pub use particles::take_particle_bursts;
pub use printed::{PrintPart, PrintValue, TableDump, TableEntry};
pub use runtime::{Reported, RunError, RunState, Runtime, Step};

/// Turns Luau's `export` statements on or off for every VM in the process
/// (Studio's "Luau Exports and Cyclic Requires" beta): `export local`,
/// `export const` and `export function` in a ModuleScript make it return
/// a table of those values, and a module that exports may be required
/// again while it still loads. Call it before any script compiles.
pub fn set_luau_exports(on: bool) {
    let _ = mlua::Lua::set_fflag("LuauExportValueSyntax", on);
    engine::require::EXPORTS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether `source` compiles as Luau (nothing runs). `Err` is the
/// compiler's message, e.g. a syntax error or "too many upvalues".
pub fn compile_check(source: &str, name: &str) -> Result<(), String> {
    luau::new_vm()
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
