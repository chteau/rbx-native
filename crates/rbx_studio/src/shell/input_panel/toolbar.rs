//! The strip above the table, the last rejected edit, and what the dock
//! shows before the place has any input context.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;

use super::super::chrome;
use super::Shell;
use crate::input_actions::Model;
use crate::tokens;

impl Shell {
    /// The strip above the table: counts, the Check toggle and New
    /// InputContext.
    pub(super) fn input_toolbar(
        &mut self,
        model: &Model,
        issue_count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let actions: usize = model
            .contexts
            .iter()
            .map(|c| c.actions.len())
            .sum::<usize>()
            + model.loose.len();
        let checking = self.input_edits.checking;
        let label = match checking {
            true => format!("Check ({issue_count})"),
            false => "Check".to_owned(),
        };
        h_flex()
            .w_full()
            .flex_none()
            .items_center()
            .gap(tokens::label_gap())
            .px(px(8.))
            .py(tokens::row_padding())
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .child(SharedString::from(format!(
                        "{} · {}",
                        counted(model.contexts.len(), "context"),
                        counted(actions, "action")
                    ))),
            )
            .child(
                chrome::button("input-check", label, checking)
                    .track_focus(&self.tab_order.claim(cx))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.input_edits.checking = !shell.input_edits.checking;
                        cx.notify();
                    })),
            )
            .child(self.new_context_button(cx))
            .into_any_element()
    }

    /// The last rejected edit, pinned above the table.
    pub(super) fn input_error(&self) -> Option<AnyElement> {
        let message = self.input_edits.error.clone()?;
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .items_center()
                .gap(tokens::label_gap())
                .px(px(8.))
                .py(tokens::row_padding())
                .bg(tokens::error_soft())
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text_error())
                .child(Icon::new(IconName::CircleAlert).size(tokens::text_sm()))
                .child(div().flex_1().min_w_0().child(SharedString::from(message)))
                .into_any_element(),
        )
    }

    fn new_context_button(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        chrome::button("input-new-context", "New InputContext", false)
            .track_focus(&self.tab_order.claim(cx))
            .on_click(cx.listener(|shell, _, _, cx| shell.add_input_context(cx)))
    }

    pub(super) fn input_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(tokens::label_gap())
            .p(tokens::panel_padding())
            .child(
                div()
                    .text_color(tokens::text_muted())
                    .child(Icon::new(IconName::Gamepad2).size(tokens::text_lg())),
            )
            .child(
                div()
                    .text_size(tokens::text_md())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text_strong())
                    .child("No input contexts yet"),
            )
            .child(
                div()
                    .text_center()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text_muted())
                    .child("An InputContext groups actions and the bindings that trigger them."),
            )
            .child(
                div()
                    .pt(tokens::label_gap())
                    .child(self.new_context_button(cx)),
            )
            .into_any_element()
    }
}

fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}
