//! Studio's Command Bar: one line at the bottom of the window that runs
//! whatever Luau it is given against the open place with `rbx_lua::Runtime`.
//!
//! Running a script is [`Shell`]'s job (see `Shell::run_command`), because it
//! is the one holding the DOM, the Explorer and the viewport; this module only
//! owns the input widget and the label above it that shows what the last run
//! did.
//!
//! `RBX_STUDIO_RUN=<source>` runs one chunk exactly as pressing Enter would,
//! once, right after the window opens — a debugging aid for a screenshot that
//! proves the bar works without sending it synthetic input (see `AGENTS.md`'s
//! safety rules), e.g.
//! `RBX_STUDIO_RUN='workspace.Baseplate.Transparency = 0.5' rbxstudio place.rbxl`.

mod feedback;
mod run;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

pub(crate) use feedback::Feedback;
pub(crate) use run::run;

/// Read once at startup by `main`; documented in this module's doc comment.
pub(crate) const RUN_VARIABLE: &str = "RBX_STUDIO_RUN";

pub(crate) struct CommandBar {
    input: Entity<InputState>,
    feedback: Feedback,
}

impl CommandBar {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        CommandBar {
            input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Command Bar — Luau, Enter to run")
            }),
            feedback: Feedback::default(),
        }
    }

    pub(crate) fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    pub(crate) fn set_feedback(&mut self, feedback: Feedback) {
        self.feedback = feedback;
    }

    /// The last run's outcome, for `--verbose`'s own report of what `--run`
    /// came back with (see `cli`).
    pub(crate) fn feedback(&self) -> &Feedback {
        &self.feedback
    }

    /// The bar as it sits at the bottom of the window: the last run's outcome
    /// above the input, full width.
    /// The bar as it sits at the bottom of the window. The design frame has
    /// no command bar, so this borrows a dock's own furniture — the 5px
    /// inset and the `chrome` field — rather than inventing a third look
    /// for the one row at the bottom of the window.
    pub(crate) fn render(&self, tab_index: isize, _: &App) -> impl IntoElement {
        let label = self.feedback.label();
        let color = if self.feedback.is_error() {
            tokens::text_error()
        } else {
            tokens::text_placeholder()
        };

        v_flex()
            .w_full()
            .flex_none()
            .p(px(5.))
            .gap(px(4.))
            // On the docks' own surface, not the window's black ground: the
            // field inside is the same `chrome` every property field is,
            // and against black it read as a strip rather than as an input.
            .bg(tokens::dock())
            .border_t(px(1.))
            .border_color(tokens::divider())
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
                    .rounded(tokens::RADIUS)
                    .bg(tokens::chrome())
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .with_size(tokens::field_size())
                            .h(tokens::input_height())
                            .tab_index(tab_index),
                    ),
            )
    }
}
