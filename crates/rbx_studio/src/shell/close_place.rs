//! File › Close Place: back to Home, asking first when there are unsaved
//! changes (Save / Don't Save / Cancel, in the launcher's dialog chrome like
//! the Roblox upload prompts).

use gpui_kit::*;

use crate::launcher::ui::{self, Weight};

use super::Shell;

impl Shell {
    /// Edits since the last save, including typing still on the script
    /// editor's debounce.
    fn has_unsaved_changes(&self) -> bool {
        self.recovery.unsaved() || self.scripts.open.values().any(|open| open.pending)
    }

    pub(crate) fn close_place(&mut self, cx: &mut Context<Self>) {
        if self.has_unsaved_changes() {
            self.close_prompt = true;
            cx.notify();
        } else {
            self.leave_place(cx);
        }
    }

    /// Escape's half of the prompt; returns whether it was open.
    pub(super) fn cancel_close_place(&mut self) -> bool {
        std::mem::take(&mut self.close_prompt)
    }

    fn leave_place(&mut self, cx: &mut Context<Self>) {
        self.close_prompt = false;
        let handle = self.window_handle;
        // Deferred: opening Home renders it, and this runs inside a `Shell`
        // update (a menu action or a click); removing the window from inside
        // its own handler is refused the same way.
        cx.defer(move |cx| {
            crate::open_home_again(cx);
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }

    pub(super) fn close_place_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.close_prompt {
            return None;
        }
        // Covers the window like the veil inside it; holds focus (see
        // `close_focus`).
        Some(
            div()
                .absolute()
                .inset_0()
                .track_focus(&self.close_focus)
                .child(ui::dialog(
                    440.,
                    ui::dialog_glyph("triangle-alert", ui::accent(), ui::wash()),
                    format!("Save changes to {}?", self.title),
                    "Your changes will be lost if you close the place without saving them."
                        .to_string(),
                    None,
                    vec![
                        ui::button(
                            "close-place-discard",
                            "Don\u{2019}t Save",
                            Weight::Ghost,
                            false,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            // Nothing to recover from a choice to discard:
                            // drops this session's recovery copy as a save
                            // would.
                            shell.saved();
                            shell.leave_place(cx);
                        }))
                        .into_any_element(),
                        ui::button("close-place-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.close_prompt = false;
                                cx.notify();
                            }))
                            .into_any_element(),
                        ui::button("close-place-save", "Save", Weight::Primary, false)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.save(cx);
                                // A failed save is in the Output dock; the prompt
                                // stays so the work is not lost.
                                if !shell.has_unsaved_changes() {
                                    shell.leave_place(cx);
                                }
                            }))
                            .into_any_element(),
                    ],
                ))
                .into_any_element(),
        )
    }
}
