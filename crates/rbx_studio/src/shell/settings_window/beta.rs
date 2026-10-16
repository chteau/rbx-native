//! The Beta features page: one switch per experimental feature, and the
//! restart banner shown while a switch awaits one.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::beta;
use crate::tokens;

use super::kit::{card, icon, text, toggle};
use super::SettingsWindow;

impl SettingsWindow {
    /// Beta features: the listed switches, or an empty state.
    pub(super) fn beta_body(&self) -> AnyElement {
        let features = beta::features();
        let pending = beta::pending();
        v_flex()
            .gap(px(22.))
            .when(pending > 0, |page| page.child(self.restart_banner(pending)))
            .child(if features.is_empty() {
                empty_state().into_any_element()
            } else {
                self.feature_list(&features).into_any_element()
            })
            .into_any_element()
    }

    fn feature_list(&self, features: &[beta::Feature]) -> impl IntoElement {
        let rows = features.iter().enumerate().map(|(i, feature)| {
            let id = feature.id;
            let on = beta::chosen(id);
            let shell = self.shell.clone();
            h_flex()
                .gap(px(14.))
                .px(px(14.))
                .py(px(12.))
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.))
                        .child(
                            text(13., 18.)
                                .font_weight(FontWeight::BOLD)
                                .child(feature.name),
                        )
                        .child(
                            text(12., 17.)
                                .text_color(tokens::text2())
                                .child(feature.description),
                        ),
                )
                .when(on != beta::enabled(id), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(10.))
                            .bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .text_size(px(10.5))
                            .child("Restart to apply"),
                    )
                })
                .child(toggle(("beta-toggle", i), on, move |_, _, cx| {
                    if let Err(error) = beta::choose(id, !on) {
                        eprintln!("could not save beta.json: {error}");
                    }
                    shell.update(cx, |_, cx| cx.notify());
                }))
        });
        card().children(rows)
    }

    fn restart_banner(&self, pending: usize) -> impl IntoElement {
        let shell = self.shell.clone();
        let message = if pending == 1 {
            "1 change applies after a restart.".to_string()
        } else {
            format!("{pending} changes apply after a restart.")
        };
        h_flex()
            .gap(px(12.))
            .pt(px(10.))
            .pb(px(10.))
            .pl(px(14.))
            .pr(px(12.))
            .items_center()
            .border_1()
            .border_color(tokens::accent_line())
            .rounded(px(8.))
            .bg(tokens::accent_soft())
            .child(
                div()
                    .text_color(tokens::check_on())
                    .child(icon("rotate-cw", 15.)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(tokens::text())
                    .child(message),
            )
            .child(
                h_flex()
                    .id("beta-restart")
                    .flex_none()
                    .h(px(28.))
                    .px(px(12.))
                    .items_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .bg(tokens::check_on())
                    .text_color(tokens::black())
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .child("Restart now")
                    .on_click(move |_, _, cx| {
                        shell.update(cx, |shell, cx| shell.request_restart(cx))
                    }),
            )
    }
}

fn empty_state() -> Div {
    v_flex()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .py(px(56.))
        .px(px(24.))
        .border_1()
        .border_dashed()
        .border_color(tokens::border2())
        .rounded(px(10.))
        .child(
            h_flex()
                .size(px(44.))
                .rounded(px(12.))
                .items_center()
                .justify_center()
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::border())
                .text_color(tokens::text2())
                .child(icon("flask-conical", 20.)),
        )
        .child(
            text(14., 19.)
                .font_weight(FontWeight::BOLD)
                .child("No beta features yet"),
        )
        .child(
            text(12.5, 19.)
                .max_w(px(420.))
                .text_center()
                .text_color(tokens::text2())
                .child(
                    "Nothing is in beta right now. Experimental features show up here, each \
                     with its own switch; they apply after a restart.",
                ),
        )
}
