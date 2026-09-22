//! The dragger guides' switches, as the viewport settings menu flips them
//! (see `crate::settings::DraggerSettings` for what each one is).

use gpui_kit::Context;

use super::Shell;
use crate::settings::DraggerSettings;

impl Shell {
    pub(super) fn dragger(&self) -> DraggerSettings {
        self.dragger
    }

    pub(super) fn set_dragger(&mut self, dragger: DraggerSettings, cx: &mut Context<Self>) {
        if dragger == self.dragger {
            return;
        }
        self.dragger = dragger;
        self.viewport
            .update(cx, |viewport, _| viewport.set_dragger(dragger));
        self.save_settings();
        cx.notify();
    }
}
