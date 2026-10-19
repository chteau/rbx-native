//! File › Close Place: back to Home, asking first when there are unsaved
//! changes (Save / Don't Save / Cancel, in the launcher's dialog chrome like
//! the Roblox upload prompts).

use std::path::PathBuf;

use gpui_kit::*;

use crate::launcher::ui::{self, Weight};

use super::Shell;

/// Where the editor goes once the unsaved-changes prompt is answered.
#[derive(Clone, Default, PartialEq)]
pub(super) enum AfterClose {
    /// File › Close Place: back to Home.
    #[default]
    Home,
    /// Settings › Beta features' Restart now.
    Restart,
    /// The titlebar's Close button or the window manager's close.
    Quit,
    /// File › New: a fresh Baseplate, created once the prompt is answered
    /// so Cancel leaves no stray file.
    New,
    /// File › Open…: a window on this place instead of Home.
    Open(PathBuf),
}

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

    /// File › New: a new Baseplate in a window of its own, replacing this
    /// one once any unsaved changes are dealt with.
    pub(crate) fn request_new(&mut self, cx: &mut Context<Self>) {
        self.leave_for(AfterClose::New, cx);
    }

    /// File › Open…, with the file already picked.
    pub(super) fn request_open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.leave_for(AfterClose::Open(path), cx);
    }

    fn leave_for(&mut self, after: AfterClose, cx: &mut Context<Self>) {
        self.after_close = after;
        if self.has_unsaved_changes() {
            self.close_prompt = true;
            cx.notify();
        } else {
            self.leave_place(cx);
        }
    }

    /// Restarts the editor on this place, asking first when there are
    /// unsaved changes.
    pub(crate) fn request_restart(&mut self, cx: &mut Context<Self>) {
        if self.has_unsaved_changes() {
            self.after_close = AfterClose::Restart;
            self.close_prompt = true;
            cx.notify();
        } else {
            crate::relaunch(&self.path);
            cx.quit();
        }
    }

    /// Closes the editor window, asking first when there are unsaved
    /// changes; answers whether it may close right now (the window manager's
    /// close asks through `on_window_should_close`).
    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.has_unsaved_changes() {
            return true;
        }
        self.after_close = AfterClose::Quit;
        self.close_prompt = true;
        cx.notify();
        false
    }

    /// Escape's half of the prompt; returns whether it was open.
    pub(super) fn cancel_close_place(&mut self) -> bool {
        self.after_close = AfterClose::Home;
        std::mem::take(&mut self.close_prompt)
    }

    fn leave_place(&mut self, cx: &mut Context<Self>) {
        self.close_prompt = false;
        self.cancel_command_run();
        let handle = self.window_handle;
        match std::mem::take(&mut self.after_close) {
            AfterClose::Restart => {
                crate::relaunch(&self.path);
                cx.quit();
                return;
            }
            AfterClose::Quit => {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
                return;
            }
            AfterClose::New => match super::open_place::create_baseplate() {
                Ok(path) => return self.open_other(path, cx),
                Err(err) => return self.warn(format!("New: {err}"), cx),
            },
            AfterClose::Open(path) => return self.open_other(path, cx),
            AfterClose::Home => {}
        }
        // Deferred: opening Home renders it, and this runs inside a `Shell`
        // update (a menu action or a click); removing the window from inside
        // its own handler is refused the same way.
        cx.defer(move |cx| {
            crate::open_home_again(cx);
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }

    /// Opens `path` in a new editor window, and only once that worked closes
    /// this one; a place that cannot be read leaves this window as it was.
    fn open_other(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let handle = self.window_handle;
        let this = cx.weak_entity();
        // Deferred like Home above: opening renders, and this window cannot
        // be removed from inside its own handler.
        cx.defer(move |cx| match crate::open_place(&path, cx) {
            Ok(()) => {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            Err(err) => {
                let _ = this.update(cx, |shell, cx| shell.warn(format!("Open: {err}"), cx));
            }
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
                    match self.after_close {
                        AfterClose::Restart => {
                            format!("Save changes to {} before restarting?", self.title)
                        }
                        AfterClose::New | AfterClose::Open(_) => {
                            format!("Save changes to {} before opening another place?", self.title)
                        }
                        _ => format!("Save changes to {}?", self.title),
                    },
                    match self.after_close {
                        AfterClose::Restart => "RbxNative restarts to apply beta features. Unsaved changes are lost if you don\u{2019}t save them.",
                        AfterClose::New | AfterClose::Open(_) => "Your changes will be lost if you open another place without saving them.",
                        _ => "Your changes will be lost if you close the place without saving them.",
                    }
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
                                shell.after_close = AfterClose::Home;
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
