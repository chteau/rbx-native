//! Studio's Command Bar: the field at the bottom of the window that runs
//! whatever Luau it is given against the open place with `rbx_lua::Runtime`.
//!
//! It takes Studio's keys: Enter starts a new line (the field grows to
//! [`MAX_ROWS`] lines, then scrolls), Ctrl+Enter runs, and Ctrl+Up/Down walk
//! the history while plain Up/Down move between lines. The text stays after
//! a run, so a command can be run again or tweaked. The field is the script
//! editor's own code editor, without its gutter: the same Luau highlighting,
//! and Shift+Alt+F formats it with StyLua (`script_editor::format`).
//!
//! Running a script is [`Shell`]'s job (see `shell::command_run`), because it
//! is the one holding the DOM, the Explorer and the viewport; this module only
//! owns the input widget, the label above it that shows what the last run
//! did, the history of what was submitted, and the pure rules the
//! shell's run loop applies (`stream`, `undo`).
//!
//! `RBX_STUDIO_RUN=<source>` runs one chunk exactly as Ctrl+Enter would,
//! once, right after the window opens, a debugging aid for a screenshot that
//! proves the bar works without sending it synthetic input, e.g.
//! `RBX_STUDIO_RUN='workspace.Baseplate.Transparency = 0.5' rbxstudio place.rbxl`.

pub(crate) mod failure;
mod feedback;
mod recall;
pub(crate) mod stream;
mod undo;

use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::Size;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::script_editor::highlight;
use crate::tokens;

use recall::Recall;

pub(crate) use feedback::Feedback;
pub(crate) use undo::RunUndo;

/// Read once at startup by `main`; documented in this module's doc comment.
pub(crate) const RUN_VARIABLE: &str = "RBX_STUDIO_RUN";

/// The most lines the field grows to before it scrolls, as many as Studio's.
const MAX_ROWS: usize = 10;

/// Studio's own placeholder, with this platform's run key.
const PLACEHOLDER: &str = if cfg!(target_os = "macos") {
    "Execute a command with ⌘↵ or use ⌘↑/↓ for history"
} else {
    "Execute a command with Ctrl+Enter or use Ctrl+Up/Down for history"
};

/// Ctrl+Up / Ctrl+Down (⌘ on macOS): `Some(true)` for the older command,
/// `Some(false)` for the newer one.
pub(crate) fn history_key(keystroke: &Keystroke) -> Option<bool> {
    let modifiers = &keystroke.modifiers;
    if !modifiers.secondary() || modifiers.shift || modifiers.alt {
        return None;
    }
    match keystroke.key.as_str() {
        "up" => Some(true),
        "down" => Some(false),
        _ => None,
    }
}

/// The field's one-line height inside its 1px border.
fn first_line_height() -> Pixels {
    tokens::input_height() - px(2.)
}

/// A slot as tall as the field's first line, so what sits beside the text
/// stays level with it however many lines the field grows to.
fn first_line() -> Div {
    div()
        .flex_none()
        .h(first_line_height())
        .flex()
        .items_center()
}

pub(crate) struct CommandBar {
    input: Entity<EditorState>,
    feedback: Feedback,
    recall: Recall,
    /// Why the last Shift+Alt+F left the field alone, with the text it was
    /// pressed on. Shown only while the field still holds that text, so the
    /// first edit after it clears it with no subscription to keep.
    format_failure: Option<(String, String)>,
}

impl CommandBar {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        CommandBar {
            input: cx.new(|cx| {
                let mut state = EditorState::new(window, cx)
                    .language(highlight::LANGUAGE)
                    .line_number(false)
                    .indent_guides(false)
                    .folding(false)
                    // Ctrl+F belongs to the script editor; a find bar inside
                    // a one-line field would have nowhere to go.
                    .searchable(false)
                    .placeholder(PLACEHOLDER);
                state.set_highlighter_factory(highlight::factory(), cx);
                state
            }),
            feedback: Feedback::default(),
            recall: Recall::default(),
            format_failure: None,
        }
    }

    pub(crate) fn input(&self) -> &Entity<EditorState> {
        &self.input
    }

    /// Remembers why formatting `text` failed, until the field changes.
    pub(crate) fn set_format_failure(&mut self, text: String, message: String) {
        self.format_failure = Some((text, message));
    }

    fn format_failure(&self, cx: &App) -> Option<SharedString> {
        let (text, message) = self.format_failure.as_ref()?;
        (self.input.read(cx).value().as_ref() == text.as_str())
            .then(|| SharedString::from(message.clone()))
    }

    /// Remembers a submitted command for Ctrl+Up/Down.
    pub(crate) fn remember(&mut self, source: &str) {
        self.recall.submit(source);
    }

    /// Ctrl+Up: the older command to show in place of `current`, if any.
    pub(crate) fn recall_older(&mut self, current: &str) -> Option<String> {
        self.recall.older(current).map(str::to_owned)
    }

    /// Ctrl+Down: the newer command, or the line Up started from.
    pub(crate) fn recall_newer(&mut self) -> Option<String> {
        self.recall.newer().map(str::to_owned)
    }

    pub(crate) fn set_feedback(&mut self, feedback: Feedback) {
        self.feedback = feedback;
    }

    /// The last run's outcome, for `--verbose`'s own report of what `--run`
    /// came back with (see `cli`).
    pub(crate) fn feedback(&self) -> &Feedback {
        &self.feedback
    }

    /// The bar as it sits at the bottom of the window. The design frame has
    /// no command bar, so this borrows a dock's own furniture, the 5px
    /// inset and the `chrome` field, rather than inventing a third look
    /// for the one row at the bottom of the window. The last run's outcome
    /// sits above the input only while `output_collapsed`: otherwise the
    /// Output dock already shows it (see [`Feedback::shown_inline`]).
    pub(crate) fn render(
        &self,
        tab_index: isize,
        output_collapsed: bool,
        cx: &App,
    ) -> impl IntoElement {
        let input = self.input.read(cx);
        let handle = input.focus_handle(cx);
        // The editor has no auto-grow of its own: one row per line, up to
        // `MAX_ROWS`, past which it scrolls.
        let rows = (input.value().matches('\n').count() + 1).min(MAX_ROWS);
        let failure = self.format_failure(cx);
        let (label, color) = match failure {
            Some(failure) => (failure, tokens::text_error()),
            None if self.feedback.shown_inline(output_collapsed) => (
                self.feedback.label(),
                if self.feedback.is_error() {
                    tokens::text_error()
                } else {
                    tokens::text_placeholder()
                },
            ),
            None => (SharedString::default(), tokens::text_placeholder()),
        };

        v_flex()
            .w_full()
            .flex_none()
            .p(px(8.))
            .gap(px(4.))
            // The window's own ground, not a dock's: the field inside it
            // is the one lighter surface, and against black it reads as an
            // inset input rather than a second dock stacked on the first.
            .bg(tokens::black())
            .border_t(px(1.))
            .border_color(tokens::border())
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .when(!label.is_empty(), |this| {
                this.child(h_flex().px(px(8.)).text_color(color).child(label))
            })
            // The same field every other input in the editor is: one
            // height, one radius, one surface. It used to be 22px tall with
            // its own padding, which made the one row people type into the
            // odd one out.
            .child(
                div()
                    .w_full()
                    .min_h(tokens::input_height())
                    .px(tokens::input_padding())
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .rounded(tokens::radius())
                    .bg(tokens::field_select())
                    .border_1()
                    .border_color(tokens::border2())
                    .track_focus(&handle)
                    .focus(|this| this.border_color(tokens::accent_line()))
                    .child(first_line().text_color(tokens::check_on()).child(">"))
                    // Monospace, as Studio's is: what goes in here is code,
                    // often several lines of it that should line up. The kit
                    // pads a multi-line editor by its Medium size whatever
                    // style it's given, so the margins take that back and
                    // centre the first line in the field's one-line height,
                    // where the prompt and the chip sit.
                    .child(
                        Editor::new(&self.input)
                            .appearance(false)
                            .bordered(false)
                            // The rows plus the kit's own padding, which
                            // the margins below take back.
                            .h(tokens::line_md() * rows as f32 + Size::Medium.input_py() * 2.)
                            .font_family(tokens::font_mono())
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .ml(-Size::Medium.input_px())
                            .my((first_line_height() - tokens::line_md()) / 2.
                                - Size::Medium.input_py())
                            .flex_1()
                            .tab_index(tab_index),
                    )
                    // The language this bar always runs, named so the
                    // field doesn't have to be typed into to learn that.
                    .child(
                        first_line().child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(tokens::radius())
                                .bg(tokens::dock())
                                .border_1()
                                .border_color(tokens::border())
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text_muted())
                                .child("Luau"),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::history_key;
    use gpui_kit::Keystroke;

    fn key(spelling: &str) -> Option<bool> {
        history_key(&Keystroke::parse(spelling).unwrap())
    }

    #[test]
    fn secondary_up_and_down_walk_the_history() {
        assert_eq!(key("secondary-up"), Some(true));
        assert_eq!(key("secondary-down"), Some(false));
    }

    #[test]
    fn plain_and_extended_arrows_are_left_to_the_field() {
        for spelling in [
            "up",
            "down",
            "shift-up",
            "secondary-shift-up",
            "secondary-alt-down",
        ] {
            assert_eq!(key(spelling), None, "{spelling}");
        }
    }
}
