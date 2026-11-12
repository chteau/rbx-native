//! Format Document (Shift+Alt+F): the script editor's active script and the
//! Command Bar, both through StyLua (`script_editor::format`).

use gpui_kit::{Context, Focusable as _, Keystroke, Window};
use rbx_dom::Ref;

use super::Shell;
use crate::script_editor::format;
use crate::shortcuts::Command;

impl Shell {
    /// Shift+Alt+F while the script editor itself has focus.
    pub(super) fn handle_format_key(
        &mut self,
        reference: Ref,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !crate::shortcuts::pressed(Command::FormatDocument, keystroke) {
            return false;
        }
        let Some(open) = self.scripts.open.get(&reference) else {
            return false;
        };
        if !open.state.focus_handle(cx).is_focused(window) {
            return false;
        }
        self.format_script(reference, window, cx);
        true
    }

    /// Replaces the script's text with StyLua's as one undo step, the cursor
    /// kept on its line. Text that doesn't parse is left alone, and why is
    /// shown in the status bar until the script next changes, and in Output.
    pub(super) fn format_script(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.scripts.open.get(&reference) else {
            return;
        };
        let state = open.state.clone();
        let text = state.read(cx).value().to_string();
        match format::format(&text) {
            Ok(formatted) => {
                self.scripts.format_failure = None;
                if formatted != text {
                    state.update(cx, |state, cx| {
                        let at = state.cursor_position();
                        state.replace_all(formatted, window, cx);
                        state.set_cursor_position(at, window, cx);
                    });
                }
            }
            Err(message) => {
                // Output too: the status bar's red line is easy to miss.
                self.output.push_warning(&message);
                self.scripts.format_failure = Some((reference, message));
            }
        }
        cx.notify();
    }

    /// Shift+Alt+F in the Command Bar; the cursor ends up at the end, where
    /// Ctrl+Enter is pressed from.
    pub(super) fn format_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.command_bar.input().clone();
        let text = input.read(cx).value().to_string();
        match format::format_command(&text) {
            Ok(formatted) if formatted == text => {}
            Ok(formatted) => {
                let end = formatted.len();
                input.update(cx, |input, cx| input.replace_all(formatted, window, cx));
                // Once the field has grown to the new line count: moved now,
                // it scrolls against the old height and hides the first line.
                window.on_next_frame(move |_, cx| {
                    input.update(cx, |input, cx| input.set_selected_range(end..end, cx));
                });
            }
            Err(message) => self.command_bar.set_format_failure(text, message),
        }
        cx.notify();
    }
}
