//! The key field and what shows under it: running, the result header,
//! and the whole-key failure cards.

use super::ui::{self, Weight};
use crate::tokens;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use rbx_cloud::ScopeCheck;

use super::*;

/// The "API key" label, the masked field with its eye, and Paste.
pub(in crate::launcher) fn field(
    check: &Entity<KeyCheck>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let state = check.read(cx);
    let running = matches!(state.status, Status::Running);
    let focused = state.focus.is_focused(window);
    let shown: SharedString = if state.secret.is_empty() {
        "".into()
    } else if state.revealed {
        state.secret.clone().into()
    } else {
        "\u{2022}"
            .repeat(state.secret.chars().count().min(44))
            .into()
    };
    let empty = state.secret.is_empty();
    let revealed = state.revealed;
    let entity = check.clone();
    v_flex()
        .gap(px(6.))
        .child(
            ui::text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::text2())
                .child("API key"),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .child(
                    h_flex()
                        .id("key-field")
                        .track_focus(&state.focus)
                        .flex_1()
                        .min_w_0()
                        .h(px(36.))
                        .items_center()
                        .gap(px(8.))
                        .pl(px(12.))
                        .pr(px(6.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(if running || focused {
                            tokens::accent_line()
                        } else {
                            tokens::border2()
                        })
                        .bg(ui::panel2())
                        .cursor_text()
                        .on_click({
                            let focus = state.focus.clone();
                            move |_, window, cx| focus.focus(window, cx)
                        })
                        .on_key_down({
                            let entity = entity.clone();
                            move |event, _, cx| {
                                if entity.update(cx, |this, cx| this.on_key(event, cx)) {
                                    cx.stop_propagation();
                                }
                            }
                        })
                        .child(
                            ui::mono(12., 16.)
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(tokens::text())
                                .when(empty, |this| {
                                    this.text_color(tokens::text3())
                                        .font_family(tokens::FONT_FAMILY_UI)
                                        .child("Paste the key you copied from the Dashboard")
                                })
                                .when(!empty, |this| this.child(shown)),
                        )
                        .when(running, |this| {
                            this.child(
                                h_flex()
                                    .gap(px(6.))
                                    .text_color(tokens::text2())
                                    .child(ui::spinner("key-field-spinner", 12.))
                                    .child(ui::text(11.5, 16.).child("Checking\u{2026}")),
                            )
                        })
                        .when(!empty && !running, |this| {
                            this.child(
                                h_flex()
                                    .id("key-reveal")
                                    .size(px(26.))
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .text_color(tokens::text2())
                                    .cursor_pointer()
                                    .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                                    .child(ui::icon(if revealed { "eye-off" } else { "eye" }, 14.))
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.revealed = !this.revealed;
                                                cx.notify();
                                            })
                                        }
                                    }),
                            )
                        }),
                )
                .child(
                    ui::icon_button(
                        "key-paste",
                        "clipboard-paste",
                        "Paste",
                        Weight::Secondary,
                        false,
                    )
                    .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.paste(cx))),
                ),
        )
}

/// What sits under the field: the running header and skeleton, the result
/// header and table, or one of the whole-key cards. `table_height` is the
/// table's fixed height (`None` fills what is left).
pub(in crate::launcher) fn result(
    check: &Entity<KeyCheck>,
    table_height: Option<f32>,
    cx: &App,
) -> Option<AnyElement> {
    let state = check.read(cx);
    let rerun = {
        let entity = check.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            entity.update(cx, |this, cx| this.run(cx))
        }
    };
    let dashboard = |id: &'static str, small: bool| {
        ui::external_button(id, "Open Creator Dashboard", Weight::Secondary, small)
            .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
    };
    Some(match &state.status {
        Status::Idle => return None,
        Status::Running => v_flex()
            .gap(px(12.))
            .child(header(
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(tokens::text2())
                    .child(ui::spinner("key-check-spinner", 16.))
                    .into_any_element(),
                "Checking your key with Roblox\u{2026}",
                "This takes a moment.".into(),
                None,
            ))
            .child(super::table::skeleton(table_height))
            .into_any_element(),
        Status::Invalid(status) => card(
            ui::status_dot("x", ui::red(), Some(ui::red_soft()), 22.),
            "Roblox didn\u{2019}t accept this key",
            format!(
                "{status} {}",
                if *status == 401 { "Unauthorized" } else { "Forbidden" }
            ),
            v_flex()
                .gap(px(10.))
                .child(ui::text(12., 18.).text_color(tokens::text2()).child("The usual causes:"))
                .child(
                    v_flex()
                        .gap(px(4.))
                        .pl(px(4.))
                        .children(
                            [
                                "Part of the key is missing. It\u{2019}s one long line; copy it again with Copy Key To Clipboard.",
                                "The key was deleted or regenerated on the Creator Dashboard.",
                                "The key only allows certain IP addresses, and this network isn\u{2019}t one of them.",
                            ]
                            .map(|line| {
                                h_flex()
                                    .items_start()
                                    .gap(px(8.))
                                    .text_color(tokens::text2())
                                    .child(ui::text(12., 18.).child("\u{2022}"))
                                    .child(ui::text(12., 18.).flex_1().child(line))
                            }),
                        ),
                )
                .into_any_element(),
            vec![
                ui::button("key-retry", "Try again", Weight::Secondary, false)
                    .on_click(rerun.clone())
                    .into_any_element(),
                dashboard("key-dashboard", false).into_any_element(),
            ],
        ),
        Status::Network => card(
            ui::status_dot("wifi-off", tokens::text2(), Some(rgba(0xFFFFFF0F)), 22.),
            "Couldn\u{2019}t reach Roblox",
            "network error".into(),
            ui::text(12., 18.)
                .text_color(tokens::text2())
                .child("Check your internet connection and try again. Your key is fine as far as we know; nothing was saved.")
                .into_any_element(),
            vec![ui::button("key-retry", "Try again", Weight::Secondary, false)
                .on_click(rerun.clone())
                .into_any_element()],
        ),
        Status::Done(checked) if !checked.report.usable => {
            let (title, meta, body) = if checked.info.expired {
                (
                    "This key has expired",
                    format!("expired {}", short_date(&checked.info.expiration_time_utc)),
                    "Roblox keeps it, but it no longer works. Set a new expiration on the Creator Dashboard, or paste another key.",
                )
            } else {
                (
                    "This key is disabled",
                    "disabled".to_string(),
                    "Enable it on the Creator Dashboard, then try again.",
                )
            };
            card(
                ui::status_dot("x", ui::red(), Some(ui::red_soft()), 22.),
                title,
                meta,
                ui::text(12., 18.).text_color(tokens::text2()).child(body).into_any_element(),
                vec![
                    dashboard("key-dashboard", false).into_any_element(),
                    ui::button("key-retry", "Try again", Weight::Secondary, false)
                        .on_click(rerun.clone())
                        .into_any_element(),
                ],
            )
        }
        Status::Done(checked) => {
            let missing: Vec<&ScopeCheck> = checked
                .report
                .checks
                .iter()
                .filter(|c| c.permission.required && !c.grant.granted())
                .collect();
            let head = if missing.is_empty() {
                header(
                    ui::status_dot("check", ui::green(), Some(ui::green_soft()), 22.)
                        .into_any_element(),
                    "This key works. You can open and publish your places.",
                    meta(checked),
                    Some(vec![ui::icon_button("key-again", "refresh-cw", "Check again", Weight::Secondary, true)
                        .on_click(rerun.clone())
                        .into_any_element()]),
                )
            } else {
                let how = missing
                    .iter()
                    .map(|c| {
                        let (system, op) = c.permission.scope.rsplit_once(':').unwrap_or((c.permission.scope, ""));
                        format!("{system} \u{2192} {}", capitalize(op))
                    })
                    .collect::<Vec<_>>()
                    .join(" and ");
                header(
                    ui::status_dot("circle-alert", ui::red(), Some(ui::red_soft()), 22.).into_any_element(),
                    if missing.len() == 1 {
                        "1 required permission is missing".to_string()
                    } else {
                        format!("{} required permissions are missing", missing.len())
                    },
                    format!("Edit the key on the Dashboard and add {how}."),
                    Some(vec![
                        ui::external_button("key-open-dashboard", "Open Dashboard", Weight::Secondary, true)
                            .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
                            .into_any_element(),
                        ui::icon_button("key-again", "refresh-cw", "Check again", Weight::Secondary, true)
                            .on_click(rerun.clone())
                            .into_any_element(),
                    ]),
                )
            };
            v_flex()
                .gap(px(12.))
                .when(table_height.is_none(), |this| this.flex_1().min_h_0())
                .child(head)
                .child(table(checked, table_height, &state.scroll))
                .into_any_element()
        }
    })
}

pub(super) fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub(super) fn header(
    glyph: AnyElement,
    title: impl Into<SharedString>,
    meta: String,
    actions: Option<Vec<AnyElement>>,
) -> Div {
    h_flex()
        .items_center()
        .gap(px(10.))
        .child(glyph)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    ui::text(12.5, 17.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(title.into()),
                )
                .child(
                    ui::text(11.5, 16.)
                        .truncate()
                        .text_color(tokens::text2())
                        .child(meta),
                ),
        )
        .when_some(actions, |this, actions| {
            this.child(h_flex().gap(px(8.)).children(actions))
        })
}

/// A whole-key failure: glyph, title, mono status, body, actions.
pub(super) fn card(
    glyph: Div,
    title: &'static str,
    status: String,
    body: AnyElement,
    actions: Vec<AnyElement>,
) -> AnyElement {
    v_flex()
        .gap(px(10.))
        .p(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .child(
            h_flex()
                .items_center()
                .gap(px(10.))
                .child(glyph)
                .child(
                    ui::text(12.5, 17.)
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(title),
                )
                .child(ui::mono(11., 15.).text_color(tokens::text3()).child(status)),
        )
        .child(body)
        .child(h_flex().gap(px(8.)).mt(px(4.)).children(actions))
        .into_any_element()
}
