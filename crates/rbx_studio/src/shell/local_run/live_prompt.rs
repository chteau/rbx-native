//! The question a place's first run on its experience's real DataStores
//! asks (Test › Data: Live): a test that saves on leave writes to the data
//! real players have, so it is never a surprise.

use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::shell::Shell;

use super::data;

/// The start that waits on the answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::shell) enum Pending {
    /// Model › Run (local).
    Run,
    /// Play (local), or with a count Server & Clients.
    Play(Option<usize>),
}

impl Shell {
    /// Holds `start` behind the question when the place would go live
    /// unconfirmed; `true` when it did.
    pub(super) fn live_data_asks(&mut self, start: Pending, cx: &mut Context<Self>) -> bool {
        if !data::live_unconfirmed(&self.path) {
            return false;
        }
        self.play.live.local.live_prompt = Some(start);
        cx.notify();
        true
    }

    /// `live`: go ahead on the real DataStores, never asking again for this
    /// place; else this place goes back to local data and the run starts
    /// on it.
    fn answer_live_prompt(&mut self, live: bool, cx: &mut Context<Self>) {
        let Some(start) = self.play.live.local.live_prompt.take() else {
            return;
        };
        if live {
            data::confirm_live(&self.path);
        } else {
            data::set_mode(&self.path, data::DataMode::Kept);
        }
        match start {
            Pending::Run => self.run_local(cx),
            Pending::Play(windows) => self.play_local_in(windows, cx),
        }
    }

    /// Escape's half of the question; returns whether it was open.
    pub(in crate::shell) fn cancel_live_prompt(&mut self) -> bool {
        self.play.live.local.live_prompt.take().is_some()
    }

    pub(in crate::shell) fn live_data_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.play.live.local.live_prompt?;
        let universe = crate::home::link_of(&self.path).map_or(0, |link| link.universe_id);
        Some(
            ui::dialog(
                460.,
                ui::dialog_glyph("triangle-alert", ui::accent(), ui::wash()),
                "Use this experience\u{2019}s live DataStores?",
                format!(
                    "Test \u{203a} Data is Live for this place, so this run reads and writes the real DataStores of experience {universe}, the data your players have, like Studio\u{2019}s Play with Enable Studio Access to API Services on. I only ask once for this place."
                ),
                None,
                vec![
                    ui::button("live-data-cancel", "Cancel", Weight::Ghost, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.cancel_live_prompt();
                            cx.notify();
                        }))
                        .into_any_element(),
                    ui::button("live-data-yes", "Use live data", Weight::Secondary, false)
                        .on_click(cx.listener(|shell, _, _, cx| shell.answer_live_prompt(true, cx)))
                        .into_any_element(),
                    ui::button("live-data-local", "Use local data", Weight::Primary, false)
                        .on_click(
                            cx.listener(|shell, _, _, cx| shell.answer_live_prompt(false, cx)),
                        )
                        .into_any_element(),
                ],
            )
            .into_any_element(),
        )
    }
}
