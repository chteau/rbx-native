//! Applying what one plugin dispatch did to the editor.

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_lua::plugin::{Outcome, OutputKind, Request, Ui, Widget};

use crate::command_bar::Feedback;
use crate::shell::Shell;

impl Shell {
    /// Applies one dispatch's outcome: changes via `rebuild_explorer` +
    /// `reflect_changes` (the undo step is `plugin_input`'s), selection via
    /// `reselect`, output lines, requests, and UI and adornment refreshes.
    ///
    /// Only an outcome that carries something redraws the editor: a plugin
    /// with a `task.wait` loop or a `Heartbeat` connection is ticked ~33
    /// times a second, and redrawing the whole window for each empty tick
    /// kept the editor busy doing nothing. A widget whose content changed
    /// redraws when its new render lands (see `widgets`), not here.
    pub(in crate::shell) fn apply_plugin_outcome(
        &mut self,
        outcome: Outcome,
        cx: &mut Context<Self>,
    ) {
        let mut redraw = !outcome.output.is_empty()
            || !outcome.requests.is_empty()
            || outcome.adornments_changed;
        if !outcome.changes.is_empty() {
            self.plugins.applying = true;
            self.rebuild_explorer(cx);
            self.reflect_changes(&outcome.changes, cx);
            self.plugins.applying = false;
            redraw = true;
        }
        if let Some(selection) = outcome.selection {
            // Recorded first so the resulting selection change is not echoed
            // back to the plugins that made it.
            self.plugins.last_selection = selection.clone();
            self.reselect(selection, cx);
            redraw = true;
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
                let old = std::mem::replace(&mut self.plugins.ui, host.ui());
                redraw |= !same_but_revisions(&old, &self.plugins.ui);
            }
            self.plugin_widgets_changed(cx);
        }
        if outcome.adornments_changed {
            self.sync_plugin_handles(cx);
        }
        if redraw {
            cx.notify();
        }
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

/// Whether `a` and `b` show the same toolbars, actions and widgets, telling
/// apart only what each widget's content revision says: a revision bump is
/// a new render to wait for, not something the editor draws itself.
fn same_but_revisions(a: &Ui, b: &Ui) -> bool {
    a.toolbars == b.toolbars
        && a.actions == b.actions
        && a.widgets.len() == b.widgets.len()
        && a.widgets.iter().zip(&b.widgets).all(|(a, b)| {
            Widget {
                revision: b.revision,
                ..a.clone()
            } == *b
        })
}

#[cfg(test)]
mod tests {
    use rbx_dom::Ref;
    use rbx_lua::plugin::{Ui, Widget, WidgetKind};

    use super::same_but_revisions;

    fn ui(revision: u64, title: &str) -> Ui {
        Ui {
            widgets: vec![Widget {
                id: 1,
                plugin: 1,
                key: "chat".into(),
                title: title.into(),
                enabled: true,
                root: Ref::new(7),
                kind: WidgetKind::Overlay { display_order: 0 },
                revision,
            }],
            ..Ui::default()
        }
    }

    #[test]
    fn a_revision_bump_alone_is_not_a_redraw() {
        assert!(same_but_revisions(&ui(1, "Chat"), &ui(9, "Chat")));
        assert!(!same_but_revisions(&ui(1, "Chat"), &ui(1, "Logs")));
        assert!(!same_but_revisions(&ui(1, "Chat"), &Ui::default()));
    }
}
