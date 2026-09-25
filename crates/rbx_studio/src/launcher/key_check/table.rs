//! The permission table: REQUIRED then OPTIONAL, one row per scope, and
//! its skeleton while a check runs.

use super::ui::{self};
use crate::tokens;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use rbx_cloud::{Grant, ScopeCheck};
use std::collections::HashMap;

use super::*;

pub(super) fn frame(height: Option<f32>) -> Stateful<Div> {
    v_flex()
        .id("permission-table")
        .map(|this| match height {
            Some(h) => this.h(px(h)),
            None => this.flex_1().min_h_0(),
        })
        .rounded(px(8.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .overflow_y_scroll()
}

pub(super) fn skeleton(height: Option<f32>) -> impl IntoElement {
    const WIDTHS: [(f32, f32); 11] = [
        (180., 150.),
        (210., 130.),
        (140., 170.),
        (200., 120.),
        (160., 190.),
        (190., 140.),
        (150., 160.),
        (220., 110.),
        (170., 150.),
        (130., 180.),
        (200., 140.),
    ];
    frame(height)
        .overflow_hidden()
        .children(WIDTHS.map(|(a, b)| {
            h_flex()
                .h(px(30.))
                .flex_none()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .border_b_1()
                .border_color(tokens::border())
                .child(div().size(px(16.)).rounded_full().bg(ui::wash()))
                .child(div().w(px(a)).h(px(9.)).rounded(px(3.)).bg(ui::wash()))
                .child(div().flex_1())
                .child(
                    div()
                        .w(px(b))
                        .h(px(9.))
                        .rounded(px(3.))
                        .bg(ui::wash_faint()),
                )
        }))
}

/// The permission table: REQUIRED then OPTIONAL, one row per scope.
pub(in crate::launcher) fn table(
    checked: &Checked,
    height: Option<f32>,
    scroll: &ScrollHandle,
) -> impl IntoElement {
    let ((req_on, req_all), (opt_on, opt_all)) = counts(&checked.report);
    let group = |label: &'static str, count: String| {
        h_flex()
            .h(px(28.))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .bg(ui::panel())
            .border_b_1()
            .border_color(tokens::border())
            .child(
                ui::text(10., 14.)
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens::text3())
                    .child(label),
            )
            .child(ui::mono(10.5, 14.).text_color(tokens::text3()).child(count))
    };
    let rows = |required: bool| {
        checked
            .report
            .checks
            .iter()
            .filter(move |c| c.permission.required == required)
            .enumerate()
            .map(|(index, c)| row(index, c, &checked.universes))
            .collect::<Vec<_>>()
    };
    frame(height)
        .track_scroll(scroll)
        .child(group("REQUIRED", format!("{req_on} of {req_all}")))
        .children(rows(true))
        .child(group(
            "OPTIONAL",
            format!("{opt_on} of {opt_all} on \u{b7} the rest stay off"),
        ))
        .children(rows(false))
        .vertical_scrollbar(scroll)
}

pub(super) fn row(index: usize, check: &ScopeCheck, names: &HashMap<u64, String>) -> AnyElement {
    let required = check.permission.required;
    let (dot, label_color, grant): (Div, Rgba, AnyElement) = match &check.grant {
        Grant::Missing if required => (
            ui::status_dot("x", ui::red(), Some(ui::red_soft()), 16.),
            tokens::text(),
            ui::text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::red())
                .child("Missing")
                .into_any_element(),
        ),
        Grant::Missing => (
            ui::status_dot("minus", tokens::text3(), None, 16.),
            tokens::text2(),
            ui::text(11.5, 16.)
                .text_color(tokens::text3())
                .child("Off")
                .into_any_element(),
        ),
        Grant::Everywhere => (
            ui::status_dot("check", ui::green(), Some(ui::green_soft()), 16.),
            tokens::text(),
            ui::text(11.5, 16.)
                .text_color(tokens::text2())
                .child("Everywhere")
                .into_any_element(),
        ),
        Grant::Universes(ids) => {
            let label = match ids.as_slice() {
                [one] => names
                    .get(one)
                    .cloned()
                    .unwrap_or_else(|| format!("Universe {one}")),
                many => format!("{} experiences", many.len()),
            };
            let tip: SharedString = ids
                .iter()
                .map(|id| {
                    names
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| format!("Universe {id}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
                .into();
            (
                ui::status_dot("check", ui::green(), Some(ui::green_soft()), 16.),
                tokens::text(),
                div()
                    .id(SharedString::from(format!(
                        "grant-{}",
                        check.permission.scope
                    )))
                    .max_w_full()
                    .tooltip(move |window, cx| crate::shell::tooltip::text(tip.clone(), window, cx))
                    .child(ui::pill(label, Some("lock"), false))
                    .into_any_element(),
            )
        }
    };
    let _ = index;
    h_flex()
        .h(px(30.))
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .border_b_1()
        .border_color(tokens::border())
        .child(dot)
        .child(
            ui::text(12., 16.)
                .w(px(230.))
                .flex_none()
                .truncate()
                .text_color(label_color)
                .child(check.permission.feature),
        )
        .child(
            ui::mono(11., 15.)
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(tokens::text3())
                .child(check.permission.scope),
        )
        .child(h_flex().w(px(150.)).flex_none().justify_end().child(grant))
        .into_any_element()
}
