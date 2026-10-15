//! rbx-native WASM plugins, run with `rbx_wasm` against the place.

use std::path::Path;

use gpui_kit::*;
use rbx_wasm::WasmPlugin;

use crate::shell::Shell;

pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) plugin: WasmPlugin,
    /// Registered commands: id, text, tooltip.
    pub(crate) commands: Vec<(String, String, String)>,
    pub(crate) error: Option<String>,
}

/// Reads `plugin.toml` + its entry (`.wasm` bytes or `.wat` text) and
/// instantiates it with the manifest's grants
/// (`rbx_lua::plugin::loader` parses the manifest).
pub(crate) fn load(folder: &Path) -> Result<Entry, String> {
    todo!()
}

impl Shell {
    /// Runs `rbxn_start` of every entry not yet started.
    pub(super) fn wasm_start(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    /// A WASM command button was clicked: runs `rbxn_command(id)` with a
    /// `rbx_wasm::DomHost` over `self.dom` and the selection, as one undo
    /// step if it changed the place; applies the effects (Output lines,
    /// commands → `self.plugins.wasm[i].commands`, notify → Info line) and
    /// a selection change.
    pub(super) fn wasm_command(&mut self, index: usize, id: String, cx: &mut Context<Self>) {
        todo!()
    }
}
