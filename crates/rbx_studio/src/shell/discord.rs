//! Shell ↔ Discord Rich Presence: start, stop, and update when the
//! active document or script tab changes.

use std::time::Duration;

use gpui_kit::Context;

use crate::discord_presence::{self, Activity, Kind, Presence};

use super::Shell;

impl Shell {
    pub(super) fn start_discord(&mut self) {
        self.discord_started = discord_presence::now_timestamp();
        self.discord = Some(Presence::start(self.discord_activity()));
    }

    pub(super) fn stop_discord(&mut self) {
        self.discord = None;
    }

    pub(super) fn update_discord(&mut self) {
        if let Some(presence) = &self.discord {
            presence.update(self.discord_activity());
        }
    }

    pub(in crate::shell) fn set_discord_presence(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            self.start_discord();
        } else {
            self.stop_discord();
        }
        self.save_settings();
        cx.notify();
    }

    pub(in crate::shell) fn set_discord_hide_names(&mut self, hide: bool, cx: &mut Context<Self>) {
        self.discord_hide_names = hide;
        self.update_discord();
        self.save_settings();
        cx.notify();
    }

    fn discord_activity(&self) -> Activity {
        let hide = self.discord_hide_names;
        let place = if hide {
            String::new()
        } else {
            self.path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
        };
        let detail = if hide {
            String::new()
        } else {
            self.discord_detail()
        };
        Activity {
            place,
            detail,
            started: self.discord_started,
            kind: self.discord_kind(),
        }
    }

    /// The main window losing focus is not idleness on its own: focus may
    /// have gone to Settings or another of the editor's windows. Idle is
    /// no editor window focused at all, checked every `IDLE_AFTER` until
    /// the main window is back, which also keeps alt-tabbing from flapping.
    pub(super) fn discord_window_activation(&mut self, active: bool, cx: &mut Context<Self>) {
        const IDLE_AFTER: Duration = Duration::from_secs(60);
        if active || self.discord.is_none() {
            self.discord_idle_check = None;
            self.set_discord_idle(false);
            return;
        }
        if self.discord_idle_check.is_some() {
            return;
        }
        self.discord_idle_check = Some(cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(IDLE_AFTER).await;
            let checked = shell.update(cx, |shell, cx| {
                shell.set_discord_idle(cx.active_window().is_none());
            });
            if checked.is_err() {
                return;
            }
        }));
    }

    fn set_discord_idle(&mut self, idle: bool) {
        if self.discord_idle != idle {
            self.discord_idle = idle;
            self.update_discord();
        }
    }

    fn discord_kind(&self) -> Kind {
        use super::chrome::Document;
        if self.discord_idle {
            return Kind::Idling;
        }
        match self.document {
            Document::Viewport => Kind::Building,
            Document::UiEditor => Kind::UiDesigning,
            Document::Scripts => Kind::Scripting,
        }
    }

    fn discord_detail(&self) -> String {
        use super::chrome::Document;
        match self.document {
            Document::Viewport => "Viewport".into(),
            Document::UiEditor => "UI Editor".into(),
            Document::Scripts => {
                let active = self.scripts.tabs.active();
                match active.and_then(|r| self.dom.get(r)) {
                    Some(inst) => inst.name().to_string(),
                    None => "Script Editor".into(),
                }
            }
        }
    }
}
