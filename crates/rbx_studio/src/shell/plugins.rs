//! Plugins: real Roblox Studio plugins (`.lua`/`.rbxm`/`.rbxmx` from
//! rbx-native's plugins folder and, when found, Studio's own) run through
//! `rbx_lua::plugin::PluginHost`, and rbx-native's own plugins
//! (`plugin.toml` with a Luau or WASM entry; WASM runs under `rbx_wasm`).
//!
//! Every input reaches the host through [`Shell::plugin_input`], which lends
//! it `self.dom` for the call and applies the `Outcome`: the change log
//! (reflected like any other edit, one undo step for a user-initiated
//! input), the selection, Output lines, requests (undo/redo, prompts,
//! opening a script, debugger control) and UI changes.

mod debugger_bridge;
mod discover;
mod handles;
mod manager;
mod prompts;
mod render;
mod ribbon;
mod wasm;
mod widgets;

use std::collections::HashSet;
use std::time::Instant;

use gpui_kit::*;
use rbx_dom::{Change, Ref};
use rbx_lua::plugin::{Input, Outcome, PluginHost, Ui};

use super::Shell;

/// Everything the editor keeps about plugins.
#[derive(Default)]
pub(crate) struct Plugins {
    /// `None` until [`Shell::plugins_start`] ran.
    pub(super) host: Option<PluginHost>,
    /// rbx-native WASM plugins.
    pub(super) wasm: Vec<wasm::Entry>,
    /// Every plugin file/folder found, enabled or not, for the manager.
    pub(super) found: Vec<discover::Found>,
    /// The host's UI as of its last change.
    pub(super) ui: Ui,
    /// The poll loop (`Input::Tick` while the host wants ticks).
    pub(super) poll: Option<Task<()>>,
    /// Set while an outcome is being applied, so the edits it reflects are
    /// not fed back to the plugins as `Input::DomChanged`.
    pub(super) applying: bool,
    /// The selection as the plugins last saw it.
    pub(super) last_selection: Vec<Ref>,
    pub(super) prompts: prompts::Prompts,
    pub(super) widgets: widgets::Widgets,
    pub(super) handles: handles::Handles,
    /// The Manage Plugins dialog is open.
    pub(super) manager_open: bool,
    /// Origin of `Context::now`.
    pub(super) epoch: Option<Instant>,
    /// Plugin paths the user switched off (persisted in settings).
    pub(super) disabled: HashSet<String>,
}

impl Shell {
    /// Discovers the installed plugins (see `discover`), adds every enabled
    /// one to a fresh host (Luau/Roblox) or `wasm` list, dispatches
    /// `Input::Start`, starts the WASM ones, and starts the poll loop
    /// (every 30 ms while `PluginHost::wants_tick` or an HTTP request is in
    /// flight; idle otherwise, re-armed by the next dispatch). Errors become
    /// Output warnings under the plugin's name; nothing here can fail the
    /// editor.
    pub(super) fn plugins_start(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    /// Unloads everything (`Input::Unload` for each plugin) and starts again.
    pub(super) fn plugins_reload(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    /// Runs one input through the host. User-initiated inputs (button,
    /// action, prompt answer, widget GUI, handle, widget closed) are one
    /// undo step when they change the place: the DOM is cloned first and
    /// pushed with `push_history_snapshot` only if the outcome has
    /// changes. Does nothing during Play (`live_shown`) or before start.
    pub(super) fn plugin_input(&mut self, input: Input, cx: &mut Context<Self>) {
        todo!()
    }

    /// Applies one dispatch's outcome (see the module docs): changes via
    /// `rebuild_explorer` + `reflect_changes` (+ `record_history_change`
    /// when a snapshot was pushed), selection via `reselect`, output via
    /// `self.output.push(source, Feedback)`, requests (`Undo`/`Redo` →
    /// `undo`/`redo`, `Prompt` → `prompts`, `OpenScript` → open the script
    /// tab at the line, `Debugger` → `debugger_bridge`, `Notify` → an Info
    /// Output line), `ui_changed` → refresh `self.plugins.ui` and the
    /// widgets, `adornments_changed` → `handles`.
    pub(super) fn apply_plugin_outcome(&mut self, outcome: Outcome, snapshot: bool, cx: &mut Context<Self>) {
        todo!()
    }

    /// From `selection_changed`: `Input::SelectionChanged` when the
    /// selection differs from `last_selection`.
    pub(super) fn plugins_selection_changed(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    /// From `reflect_changes`: `Input::DomChanged` when not `applying` and
    /// `PluginHost::watches(changes)`.
    pub(super) fn plugins_dom_changed(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        todo!()
    }

    /// Painted at the window root: the open prompt, floating widgets and
    /// the Manage Plugins dialog.
    pub(super) fn plugin_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        todo!()
    }
}
