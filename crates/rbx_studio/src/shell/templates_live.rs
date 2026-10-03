//! Keeping the user's script templates current while the editor runs: the
//! Model menu and the ribbon's Script menu list them, and both must follow
//! a template added, renamed or deleted — from the templates window or by
//! hand — without a restart.

use gpui_kit::*;

use super::Shell;
use crate::script_templates::ScriptTemplates;

impl Shell {
    /// Polls the templates folder's fingerprint for as long as the window
    /// lives — the same approach, and the same interval, as the theme
    /// watcher (see `theme::watch` for why it polls).
    pub(super) fn watch_script_templates(&mut self, cx: &mut Context<Self>) {
        self.templates_stamp = templates_stamp(&self.script_templates);
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor()
                .timer(crate::theme::POLL_INTERVAL)
                .await;
            let alive = shell.update(cx, |shell, cx| {
                if templates_stamp(&shell.script_templates) != shell.templates_stamp {
                    shell.reload_script_templates(cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Reads the templates folder again and rebuilds the menus from it.
    /// Called straight after every write the editor makes itself, so its
    /// own changes never wait for the poll.
    pub(crate) fn reload_script_templates(&mut self, cx: &mut Context<Self>) {
        let templates = ScriptTemplates::load();
        self.templates_stamp = templates_stamp(&templates);
        if templates != self.script_templates {
            crate::menu_bar::refresh(&self.menu_bar, templates.extras(), cx);
            self.script_templates = templates;
        }
        cx.notify();
    }
}

fn templates_stamp(templates: &ScriptTemplates) -> u64 {
    crate::theme::fingerprint(templates.dir())
}
