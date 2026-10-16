//! Applying what one plugin dispatch did to the editor.

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_lua::plugin::{Outcome, OutputKind, Request};

use crate::command_bar::Feedback;
use crate::shell::Shell;

impl Shell {
    /// Applies one dispatch's outcome: changes via `rebuild_explorer` +
    /// `reflect_changes` (the undo step is `plugin_input`'s), selection via `reselect`, output lines, requests, and UI and
    /// adornment refreshes.
    pub(in crate::shell) fn apply_plugin_outcome(
        &mut self,
        outcome: Outcome,
        cx: &mut Context<Self>,
    ) {
        if !outcome.changes.is_empty() {
            self.plugins.applying = true;
            self.rebuild_explorer(cx);
            self.reflect_changes(&outcome.changes, cx);
            self.plugins.applying = false;
        }
        if let Some(selection) = outcome.selection {
            // Recorded first so the resulting selection change is not echoed
            // back to the plugins that made it.
            self.plugins.last_selection = selection.clone();
            self.reselect(selection, cx);
        }
        for line in outcome.output {
            let feedback = match line.kind {
                OutputKind::Print => Feedback::Output(line.text),
                OutputKind::Info => Feedback::Info(line.text),
                OutputKind::Warn => Feedback::Warning(line.text),
                OutputKind::Error => Feedback::Error(line.text),
            };
            self.output.push_plugin(&line.source, feedback);
        }
        for request in outcome.requests {
            self.plugin_request(request, cx);
        }
        if outcome.ui_changed {
            if let Some(host) = &self.plugins.host {
                self.plugins.ui = host.ui();
            }
            self.plugin_widgets_changed(cx);
        }
        if outcome.adornments_changed {
            self.sync_plugin_handles(cx);
        }
        cx.notify();
    }

    fn plugin_request(&mut self, request: Request, cx: &mut Context<Self>) {
        match request {
            Request::Undo => self.undo(cx),
            Request::Redo => self.redo(cx),
            Request::Waypoint(_) => self.commit_plugin_step(),
            Request::Prompt { id, plugin, kind } => self.plugin_prompt(id, plugin, kind, cx),
            Request::OpenScript { script, .. } => self.plugin_open_script(script, cx),
            Request::Debugger(request) => self.plugin_debugger_request(request, cx),
            Request::Notify { plugin, text } => {
                let name = self.plugin_name(plugin);
                self.output.push_plugin(&name, Feedback::Info(text));
            }
        }
    }

    fn plugin_name(&self, plugin: rbx_lua::plugin::PluginId) -> String {
        self.plugins
            .host
            .as_ref()
            .and_then(|host| host.plugins().into_iter().find(|p| p.id == plugin))
            .map(|p| p.name)
            .unwrap_or_else(|| "Plugin".to_string())
    }

    /// Opening a tab needs the window, which an outcome is applied without,
    /// so it happens on the next turn of the main window.
    fn plugin_open_script(&mut self, script: Ref, cx: &mut Context<Self>) {
        let Some(handle) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return;
        };
        let shell = cx.entity();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.open_script(script, window, cx));
            });
        });
    }
}
