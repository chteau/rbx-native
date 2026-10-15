//! Building one plugin's VM and running its scripts.

use std::rc::Rc;

use mlua::{Lua, Result};
use rbx_dom::Ref;

use super::env::Env;

/// A sandboxed Luau VM for one plugin: the command bar's globals
/// (`runtime::install_globals` — `print` there is replaced so output goes
/// to `Shared::output` as the plugin's `Print` lines), plus `warn`
/// (`Warn`), `require`, `typeof`-compatible datatypes,
/// `scheduler::install`, `plugin_object::install`,
/// `services::install_settings`, `debugger_api::install`,
/// `native::install`; `Env` as app data and `crate::hooks::HookSlot(
/// PluginHooks)`; compiled with `game`/`workspace`/`Workspace`/`script`
/// mutable; an interrupt that errors with "Script timeout: exhausted
/// allowed execution time" once `Shared::deadline` has passed. Sandboxed
/// last.
pub(crate) fn build(env: Rc<Env>) -> Result<Lua> {
    todo!()
}

/// Runs a `Script`'s `Source` in a new thread (`scheduler::spawn`), as a
/// chunk named after the script's full name, in an environment whose
/// `script` is that instance (other names fall through to the globals).
/// A compile error is reported to the Output like a runtime one.
pub(crate) fn run_script(lua: &Lua, script: Ref) -> Result<()> {
    todo!()
}

/// `require(moduleScript)`: runs the module once per VM (cached in
/// `Env::modules`) with its own `script`; it must return exactly one
/// value. `require(number)` is an error ("require by asset id is not
/// supported").
pub(crate) fn require(lua: &Lua, module: Ref) -> Result<mlua::Value> {
    todo!()
}
