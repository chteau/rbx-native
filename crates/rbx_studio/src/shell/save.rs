//! Ctrl+S at the window level (see `Render for Shell`, which wires
//! [`Shell::handle_shell_key`] on the outer container rather than on one
//! panel like `shell::keys` does for the Explorer — a save must go through no
//! matter which widget currently holds focus). `handle_shell_key` is also the
//! dispatch point `shell::history` reuses for Ctrl+Z/Ctrl+Y, `shell::group`
//! for Ctrl+G/Ctrl+Shift+G, and `shell::clipboard` for Ctrl+C/V/D, rather
//! than a second `on_key_down`.

use gpui_kit::{Context, Keystroke, Window};

use crate::command_bar::Feedback;
use crate::save::{self, Action};

use super::output::OutputLog;
use super::Shell;

impl Shell {
    /// The window-level `on_key_down` handler: Ctrl+S here, Ctrl+Z/Ctrl+Y
    /// delegated to `shell::history` (see this module's doc comment).
    pub(super) fn handle_shell_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Every keystroke cancels a bare Alt tap, including this one: Alt+S
        // must not leave a menu open behind it (see `menu_bar::alt_tap`).
        let menu_bar = self.menu_bar.clone();
        menu_bar.update(cx, |bar, cx| {
            bar.interrupt_alt_tap();
            // F10 is the other, explicit way in — and out again. Handled here
            // rather than as a binding because the bar's own key context only
            // covers the bar, and F10 has to work from wherever focus is.
            if keystroke.key == "f10" && !keystroke.modifiers.modified() {
                bar.toggle_entry(window, cx);
            }
        });
        if let Some(Action::Save) = save::action_for(&keystroke.key, keystroke.modifiers) {
            self.save(cx);
        }
        if let Some(scale) = crate::scale::action_for_keystroke(keystroke) {
            self.set_font_scale(scale.apply(crate::tokens::font_scale()), cx);
        }
        // No keyboard traps: Escape closes whichever menu is open, from
        // anywhere, and this handler sits on the window's own root so it
        // cannot be out of reach of one (WCAG 2.1.2).
        if keystroke.key == "escape" && self.open_menu.take().is_some() {
            cx.notify();
        }
        // The same key backs out of a drag in flight — an Explorer row being
        // carried to a new parent — so a drag started by mistake is never
        // committed by letting go of it. With no active drag there is nothing
        // to stop, and a bare Escape does nothing here.
        if keystroke.key == "escape" && cx.stop_active_drag(window) {
            cx.notify();
        }
        self.handle_history_key(keystroke, window, cx);
        self.handle_group_key(keystroke, cx);
        self.handle_clipboard_key(keystroke, cx);
    }

    /// Writes the current DOM back to the file it was opened from, in the
    /// format it was opened in. `pub(crate)`: also `menu_bar`'s Save item's
    /// entry point, so a menu click runs the exact same path Ctrl+S does.
    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.write_to(&path, cx);
    }

    /// `RBX_STUDIO_SAVE_AS=<path>`: documented in `save`'s module doc
    /// comment. Applied once, after `Shell::apply_debug_explorer_action`
    /// already ran (see `Shell::new`), so a script can prove Ctrl+S
    /// round-trips whatever a debug var just mutated, into a scratch path
    /// rather than overwriting the real fixture that was opened.
    pub(super) fn apply_debug_save(&mut self, cx: &mut Context<Self>) {
        if let Ok(scratch) = std::env::var(save::SAVE_AS_VARIABLE) {
            self.write_to(std::path::Path::new(&scratch), cx);
        }
    }

    /// Serializes `self.dom` in `self.format` and writes it to `path`,
    /// reporting the outcome as a row in the Output dock and through the
    /// Command Bar's own feedback label (see [`record_outcome`]). On failure
    /// the on-disk file at `path` is left exactly as it was (see
    /// `save::save`).
    fn write_to(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        // An open script editor's text reaches the DOM on a debounce (see
        // `shell::scripts`); saving must write what is on screen, not what
        // the DOM happened to hold when typing last paused.
        self.flush_script_edits(cx);
        let result = save::save(&self.dom, self.format, path);
        let feedback = record_outcome(&mut self.output, result, path);
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

/// Turns a save's result into feedback and logs it in the Output dock.
///
/// The log is not optional: the Command Bar's label is hidden while the dock
/// is open (`Feedback::shown_inline`), so a save that reported only through
/// the label would be invisible — including a failed one, which would leave
/// the user believing the file was written.
fn record_outcome(
    output: &mut OutputLog,
    result: Result<(), String>,
    path: &std::path::Path,
) -> Feedback {
    let feedback = match result {
        Ok(()) => Feedback::Output(format!("Saved {}", path.display())),
        Err(message) => Feedback::Error(message),
    };
    output.push(SOURCE, feedback.clone());
    feedback
}

/// The `source` a save's row carries, where a Command Bar run's carries the
/// command that was typed.
const SOURCE: &str = "Save";

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::command_bar::Feedback;
    use crate::shell::output::{OutputFilter, OutputLog};

    use super::record_outcome;

    /// The Command Bar hides its own label while the Output dock is open
    /// (`Feedback::shown_inline`), on the premise that every outcome is a row
    /// there. A save is the one caller that reported through the label
    /// alone, so this pins the premise: a failed save has to be findable in
    /// the log, as an error, however the dock is configured.
    #[test]
    fn a_failed_save_is_a_row_in_the_output_log() {
        let mut log = OutputLog::default();

        let feedback = record_outcome(&mut log, Err("disk full".into()), Path::new("place.rbxl"));

        assert!(feedback.is_error());
        let errors: Vec<_> = log.filtered(OutputFilter::Errors, "").collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].source(), "Save");
        assert!(!feedback.shown_inline(false), "the label is off by default");
    }

    #[test]
    fn a_successful_save_is_a_row_in_the_output_log() {
        let mut log = OutputLog::default();

        record_outcome(&mut log, Ok(()), Path::new("place.rbxl"));

        let rows: Vec<_> = log.filtered(OutputFilter::Output, "place.rbxl").collect();
        assert_eq!(rows.len(), 1, "the row names the file that was written");
    }

    #[test]
    fn the_returned_feedback_is_what_the_command_bar_shows() {
        let mut log = OutputLog::default();
        let feedback = record_outcome(&mut log, Ok(()), Path::new("a.rbxl"));
        assert!(matches!(feedback, Feedback::Output(text) if text == "Saved a.rbxl"));
    }
}
