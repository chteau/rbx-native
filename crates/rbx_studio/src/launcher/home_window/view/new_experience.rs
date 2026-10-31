//! The game picker's New experience dialog.

use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::select::Select;
use gpui_kit::component::Size;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::launcher::home_window::new_experience::FieldError;
use crate::launcher::home_window::HomeWindow;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

fn label(text: &'static str) -> Div {
    ui::text(11.5, 17.).text_color(tokens::text2()).child(text)
}

fn problem(text: impl Into<SharedString>) -> Div {
    ui::text(11.5, 17.).text_color(ui::red()).child(text.into())
}

impl HomeWindow {
    pub(super) fn new_experience_dialog(
        &self,
        busy: bool,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let form = &self.new_experience;
        let field_problem = |error: Option<FieldError>| {
            error.map(|error| problem(error.message()).into_any_element())
        };
        let body = v_flex()
            .mt(px(14.))
            .mx(px(20.))
            .gap(px(12.))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(label("Name"))
                    .child(
                        ui::field_frame(None, ui::panel2(), tokens::border(), "pencil")
                            .w_full()
                            .child(
                                div().flex_1().min_w_0().h_full().child(
                                    Input::new(&form.name)
                                        .appearance(false)
                                        .h_full()
                                        .px(px(0.))
                                        .text_size(px(12.))
                                        .text_color(tokens::text()),
                                ),
                            ),
                    )
                    .children(field_problem(form.errors.name)),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(label("Description"))
                    .child(
                        // The text starts where the one-line fields' text
                        // does, past their glyph; the editor pads itself by
                        // its Medium size whatever it's given, so the margin
                        // takes that back.
                        div()
                            .w_full()
                            .py(px(4.))
                            .pl(px(10. + 13. + 8.))
                            .pr(px(10.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(tokens::border())
                            .bg(ui::panel2())
                            .child(
                                Textarea::new(&form.description)
                                    .appearance(false)
                                    .ml(-Size::Medium.input_px())
                                    .text_size(px(12.))
                                    .text_color(tokens::text()),
                            ),
                    )
                    .children(field_problem(form.errors.description)),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.))
                            .child(label("Creator"))
                            .when(form.loading_groups, |this| {
                                this.child(
                                    div()
                                        .text_color(tokens::text3())
                                        .child(ui::spinner("creator-groups", 12.)),
                                )
                            }),
                    )
                    .child(
                        ui::field_frame(None, ui::panel2(), tokens::border(), "users")
                            .w_full()
                            .child(
                                ui::select_slot().child(
                                    Select::new(&form.creator)
                                        .appearance(false)
                                        .text_color(tokens::text())
                                        .w_full()
                                        .py_0()
                                        .px_0()
                                        .text_size(px(12.))
                                        .icon(ui::icon("chevron-down", 12.))
                                        .accessibility_label("Creator"),
                                ),
                            ),
                    )
                    .children(
                        form.groups_error
                            .as_ref()
                            .map(|reason| problem(reason.clone()).into_any_element()),
                    ),
            )
            .children(error.map(|error| {
                div()
                    .py(px(10.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(ui::red_line())
                    .bg(ui::red_soft())
                    .text_size(px(11.5))
                    .line_height(px(17.))
                    .text_color(tokens::text())
                    .child(error.to_string())
            }))
            .into_any_element();
        let create = if busy {
            ui::disabled_button("new-experience-create", "Creating\u{2026}").into_any_element()
        } else {
            ui::button("new-experience-create", "Create", Weight::Primary, false)
                .on_click(cx.listener(|this, _, _, cx| this.confirm_new_experience(cx)))
                .into_any_element()
        };
        ui::dialog(
            480.,
            ui::dialog_glyph("plus", ui::accent(), tokens::accent_soft()),
            "Create new experience",
            "Roblox makes a private experience with one start place, then this file is published to it.",
            Some(body),
            vec![
                ui::button("new-experience-cancel", "Cancel", Weight::Secondary, false)
                    .when(!busy, |this| {
                        this.on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)))
                    })
                    .into_any_element(),
                create,
            ],
        )
    }
}
