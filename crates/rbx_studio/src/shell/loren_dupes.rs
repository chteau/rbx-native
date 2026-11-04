//! Loren's duplicate-name warnings in the Script Analysis dock and the
//! editor squiggles. Unlike `luau-lsp` they read the DOM alone, so they show
//! with the server missing or failed.

use std::time::Duration;

use gpui_kit::*;

use crate::luau_lsp::diagnostics;

use super::Shell;

/// Typing in a script moves the history revision per edit; wait for a pause.
const RECHECK_DELAY: Duration = Duration::from_millis(500);

impl Shell {
    /// Called from a render: schedules a recheck when the edit DOM moved since
    /// the last one. Play's live views are not edited, so they are left out.
    pub(super) fn notice_loren_dom_change(&mut self, cx: &mut Context<Self>) {
        if self.live_shown() || self.lsp.loren_scheduled {
            return;
        }
        if self.lsp.loren_revision == Some(self.history.revision()) {
            return;
        }
        self.lsp.loren_scheduled = true;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(RECHECK_DELAY).await;
            let _ = shell.update(cx, |shell, cx| shell.recheck_loren(cx));
        })
        .detach();
    }

    fn recheck_loren(&mut self, cx: &mut Context<Self>) {
        self.lsp.loren_scheduled = false;
        self.lsp.loren_revision = Some(self.history.revision());
        let place = super::play::live::edit_dom_of(&self.dom, &self.play.live);
        self.lsp.loren = diagnostics::loren_problems(place);
        self.show_problems(cx);
    }
}
