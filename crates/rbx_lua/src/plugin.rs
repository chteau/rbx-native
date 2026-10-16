//! A host for real Roblox Studio plugins — and for rbx-native's own Luau
//! plugins, which run through the same host with one extra global.
//!
//! Each plugin gets its own sandboxed Luau VM that lives as long as the host,
//! so a toolbar button's `Click` handler, a `Selection.SelectionChanged`
//! connection or a `task.delay` keep running after the plugin's script has
//! returned. Only an allow-listed subset of the plugin API exists
//! (`Plugin`/`PluginToolbar`, `Selection`, `ChangeHistoryService`,
//! `HttpService`, `RunService`, signals, `task`), plus the extensions this
//! project adds on `plugin` (`PromptColor3`, `PromptBrickColor`,
//! `CreateFloatingWidget`, `GetDebuggerManager`).
//!
//! The place's DOM is owned by the editor. [`PluginHost::dispatch`] borrows
//! it for the length of one input: the host's own instances (each plugin's
//! script tree, its widgets, its `CoreGui` adornments, anything it created
//! and left unparented) are *parked* outside it between dispatches and merged
//! back in for the next one (see `lot`), so they never reach the Explorer,
//! the saved file or the undo history, while `Adornee`/`Value` references
//! across the two still resolve.

mod api;
mod arg;
mod debugger_api;
mod env;
mod events;
mod guard;
mod host;
mod http;
mod input;
mod loader;
mod lot;
mod native;
mod plugin_object;
mod require;
mod scheduler;
mod services;
mod signal;
mod undo;
mod vm;
mod world;

#[cfg(test)]
mod testing;

pub use api::*;
pub use host::PluginHost;
pub use loader::{copy_tree, read_source};
