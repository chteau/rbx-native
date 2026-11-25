//! rbx-native WASM plugins, run with `rbx_wasm` against the place.

use std::path::Path;

use gpui_kit::*;
use rbx_dom::WeakDom;
use rbx_permissions::{Capability, PluginKind, PluginProfile};
use rbx_wasm::{DomHost, Effect, Grants, WasmPlugin};

use crate::command_bar::Feedback;
use crate::shell::Shell;

pub(crate) struct Entry {
    pub(crate) name: String,
    /// What its manifest asked for, for the permission store.
    pub(crate) profile: PluginProfile,
    pub(crate) plugin: WasmPlugin,
    /// Registered commands: id, text, tooltip.
    pub(crate) commands: Vec<(String, String, String)>,
    pub(crate) error: Option<String>,
    pub(crate) started: bool,
}

/// Reads `plugin.toml` + its entry (`.wasm` bytes or `.wat` text) and
/// instantiates it; what it may do is read from the permission store before
/// every call (see `wasm_run`).
pub(crate) fn load(folder: &Path) -> Result<Entry, String> {
    let manifest_path = folder.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let table: toml::Table = text.parse().map_err(|e| format!("plugin.toml: {e}"))?;
    let name = table
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let entry = table
        .get("entry")
        .and_then(|v| v.as_str())
        .ok_or("plugin.toml: missing `entry`")?;
    let mut profile = PluginProfile {
        name: name.clone(),
        kind: PluginKind::Wasm,
        ..Default::default()
    };
    let listed = table.get("permissions").and_then(|v| v.as_array());
    for text in listed.into_iter().flatten().filter_map(|p| p.as_str()) {
        match rbx_permissions::parse_manifest(text) {
            Some(rbx_permissions::Request::Capability(capability)) => {
                profile.requested.push(capability)
            }
            Some(rbx_permissions::Request::Host(host)) => profile.hosts.push(host),
            None => return Err(format!("plugin.toml: unknown permission `{text}`")),
        }
    }
    let entry_path = folder.join(entry);
    let bytes = std::fs::read(&entry_path).map_err(|e| format!("{}: {e}", entry_path.display()))?;
    let plugin = WasmPlugin::load(&name, &bytes, Grants::default())?;
    Ok(Entry {
        name,
        profile,
        plugin,
        commands: Vec::new(),
        error: None,
        started: false,
    })
}

impl Shell {
    /// Runs `rbxn_start` of every entry not yet started.
    pub(super) fn wasm_start(&mut self, cx: &mut Context<Self>) {
        for index in 0..self.plugins.wasm.len() {
            if self.plugins.wasm[index].started {
                continue;
            }
            self.plugins.wasm[index].started = true;
            self.wasm_run(index, None, cx);
        }
    }

    /// A WASM command button was clicked: runs `rbxn_command(id)` with a
    /// `rbx_wasm::DomHost` over `self.dom` and the selection, as one undo
    /// step if it changed the place; applies the effects (Output lines,
    /// commands → `self.plugins.wasm[i].commands`, notify → Info line) and
    /// a selection change.
    pub(super) fn wasm_command(&mut self, index: usize, id: String, cx: &mut Context<Self>) {
        if self.live_shown() || index >= self.plugins.wasm.len() {
            return;
        }
        self.wasm_run(index, Some(id), cx);
    }

    fn wasm_run(&mut self, index: usize, command: Option<String>, cx: &mut Context<Self>) {
        // Edits made before this run were already reflected; only the
        // plugin's own belong to its undo step.
        self.dom.take_changes();
        let before = self.dom.clone();
        let mut selection = self.selected_all().to_vec();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let entry = &mut self.plugins.wasm[index];
        let scripts = {
            let store = self.permissions.borrow();
            let policy = store.policy();
            let allows = |capability| policy.allows(&entry.profile, capability);
            entry.plugin.set_grants(Grants {
                dom_read: allows(Capability::ReadPlace),
                dom_write: allows(Capability::EditPlace),
                selection: allows(Capability::Selection),
            });
            allows(Capability::EditScripts)
        };
        let (result, selection_changed) = {
            let mut host = DomHost {
                dom: &mut dom,
                database: &self.database,
                selection: &mut selection,
                selection_changed: false,
                scripts,
            };
            let result = match &command {
                Some(id) => entry.plugin.command(id, &mut host),
                None => entry.plugin.start(&mut host),
            };
            (result, host.selection_changed)
        };
        self.dom = dom;
        let name = entry.name.clone();
        match result {
            Ok(effects) => {
                for effect in effects {
                    match effect {
                        Effect::Log(text) => self.output.push_plugin(&name, Feedback::Output(text)),
                        Effect::Warn(text) => {
                            self.output.push_plugin(&name, Feedback::Warning(text))
                        }
                        Effect::Notify(text) => {
                            self.output.push_plugin(&name, Feedback::Info(text))
                        }
                        Effect::Command { id, text, tooltip } => {
                            let commands = &mut self.plugins.wasm[index].commands;
                            commands.retain(|(existing, _, _)| *existing != id);
                            commands.push((id, text, tooltip));
                        }
                    }
                }
            }
            Err(message) => {
                self.output
                    .push_plugin(&name, Feedback::Error(message.clone()));
                self.plugins.wasm[index].error = Some(message);
            }
        }
        let changes = self.dom.take_changes();
        if !changes.is_empty() {
            self.push_history_snapshot(before);
            self.plugins.applying = true;
            self.rebuild_explorer(cx);
            self.reflect_changes(&changes, cx);
            self.plugins.applying = false;
            self.record_history_change(changes);
        }
        if selection_changed {
            self.reselect(selection, cx);
        }
        cx.notify();
    }
}
