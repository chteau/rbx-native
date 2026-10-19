//! Studio's Command Bar: one line at the bottom of the window that runs
//! whatever Luau it is given against the open place with `rbx_lua::Runtime`.
//!
//! Running a script is [`Shell`]'s job (see `shell::command_run`), because it
//! is the one holding the DOM, the Explorer and the viewport; this module only
//! owns the input widget, the label above it that shows what the last run
//! did, the Up/Down history of what was submitted, and the pure rules the
//! shell's run loop applies (`stream`, `undo`).
//!
//! `RBX_STUDIO_RUN=<source>` runs one chunk exactly as pressing Enter would,
//! once, right after the window opens — a debugging aid for a screenshot that
//! proves the bar works without sending it synthetic input (see `AGENTS.md`'s
//! safety rules), e.g.
//! `RBX_STUDIO_RUN='workspace.Baseplate.Transparency = 0.5' rbxstudio place.rbxl`.

mod feedback;
mod recall;
pub(crate) mod stream;
mod undo;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use recall::Recall;

pub(crate) use feedback::Feedback;
pub(crate) use undo::RunUndo;

/// Read once at startup by `main`; documented in this module's doc comment.
pub(crate) const RUN_VARIABLE: &str = "RBX_STUDIO_RUN";

pub(crate) struct CommandBar {
    input: Entity<InputState>,
    feedback: Feedback,
    recall: Recall,
}

impl CommandBar {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        CommandBar {
            input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Command Bar — Luau, Enter to run")
            }),
            feedback: Feedback::default(),
            recall: Recall::default(),
        }
    }

    pub(crate) fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Remembers a submitted command for Up/Down.
    pub(crate) fn remember(&mut self, source: &str) {
        self.recall.submit(source);
    }

    /// Up: the older command to show in place of `current`, if any.
    pub(crate) fn recall_older(&mut self, current: &str) -> Option<String> {
        self.recall.older(current).map(str::to_owned)
    }

    /// Down: the newer command, or the line Up started from.
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
    /// no command bar, so this borrows a dock's own furniture — the 5px
    /// inset and the `chrome` field — rather than inventing a third look
    /// for the one row at the bottom of the window. The last run's outcome
    /// sits above the input only while `output_collapsed` — otherwise the
    /// Output dock already shows it (see [`Feedback::shown_inline`]).
    pub(crate) fn render(
        &self,
        tab_index: isize,
        output_collapsed: bool,
        cx: &App,
    ) -> impl IntoElement {
        let handle = self.input.read(cx).focus_handle(cx);
        let label = if self.feedback.shown_inline(output_collapsed) {
            self.feedback.label()
        } else {
            SharedString::default()
        };
        let color = if self.feedback.is_error() {
            tokens::text_error()
        } else {
            tokens::text_placeholder()
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
                    .h(tokens::input_height())
                    .px(tokens::input_padding())
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(tokens::radius())
                    .bg(tokens::field_select())
                    .border_1()
                    .border_color(tokens::border2())
                    .track_focus(&handle)
                    .focus(|this| this.border_color(tokens::accent_line()))
                    .child(div().text_color(tokens::check_on()).child(">"))
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .with_size(tokens::field_size())
                            .h(tokens::input_height())
                            .flex_1()
                            .tab_index(tab_index),
                    )
                    // The language this bar always runs, named so the
                    // field doesn't have to be typed into to learn that.
                    .child(
                        div()
                            .flex_none()
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
            )
    }
}
