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
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

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

    /// The bar as it sits at the bottom of the window: the last run's outcome
    /// above the input, full width.
    pub(crate) fn render(&self, cx: &App) -> impl IntoElement {
        let label = self.feedback.label();
        let color = if self.feedback.is_error() {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        };

        v_flex()
            .w_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(!label.is_empty(), |this| {
                this.child(
                    h_flex()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .text_color(color)
                        .child(label),
                )
            })
            .child(div().px_2().py_1().child(Input::new(&self.input).small()))
    }
}
