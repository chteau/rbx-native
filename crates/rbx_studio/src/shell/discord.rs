//! Shell ↔ Discord Rich Presence: start, stop, and update when the
//! active document or script tab changes.

use gpui_kit::Context;

use crate::discord_presence::{self, Activity, Presence};

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

    pub(in crate::shell) fn set_discord_presence(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.start_discord();
        } else {
            self.stop_discord();
        }
        self.save_settings();
        cx.notify();
    }

    pub(in crate::shell) fn set_discord_hide_names(
        &mut self,
        hide: bool,
        cx: &mut Context<Self>,
    ) {
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
                    Some(inst) => inst.name.clone(),
                    None => "Script Editor".into(),
                }
            }
        }
    }
}
