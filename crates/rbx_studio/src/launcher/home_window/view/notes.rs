//! My Games' notes and the add-by-link row: the partial-listing warning,
//! the group-off note, the empty state.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::launcher::home_window::{HomeWindow, LinkState};
use crate::launcher::ui::{self, Weight};
use crate::tokens;

impl HomeWindow {
    pub(super) fn partial_note(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap(px(12.))
            .pt(px(12.))
            .px(px(14.))
            .pb(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(div().pt(px(1.)).text_color(tokens::warning()).child(ui::icon("triangle-alert", 16.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(12.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(1.))
                                    .child(
                                        ui::text(12.5, 17.)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(tokens::text())
                                            .child("Private experiences are hidden"),
                                    )
                                    .child(ui::text(12., 17.).text_color(tokens::text2()).child(
                                        "Your key can\u{2019}t read your inventory, so Roblox only lists your public experiences. Add Inventory \u{2192} Read to the key on the Creator Dashboard, then check it again. Or add one game by its place ID or link.",
                                    )),
                            )
                            .child(
                                ui::button("note-manage", "Manage key", Weight::Secondary, true)
                                    .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                            ),
                    )
                    .child(self.link_row(cx)),
            )
    }

    /// The 420 px link field, Add, and the error line under them.
    pub(super) fn link_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.link_state.clone();
        let resolving = state == LinkState::Resolving;
        let value = self.link.read(cx).value();
        let empty = value.trim().is_empty();
        let ellipsized = !empty && (!self.link_focused || resolving);
        let border = match state {
            LinkState::Resolving => tokens::accent_line(),
            LinkState::Idle => tokens::border(),
            _ => ui::red_line(),
        };
        let error: Option<(String, bool)> = match state {
            LinkState::NotALink => Some(("That isn\u{2019}t a place ID or a Roblox game link.".into(), false)),
            LinkState::NoPlace(id) => Some((format!("No place with ID {id}."), false)),
            LinkState::NoAccess => Some((
                "Your key can\u{2019}t open this place. It isn\u{2019}t yours, or the key is restricted to other experiences.".into(),
                true,
            )),
            LinkState::Unreachable => Some(("Couldn\u{2019}t reach Roblox. Check your connection and try again.".into(), false)),
            _ => None,
        };
        v_flex()
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        ui::field_frame(Some(420.), ui::panel(), border, "link")
                            .child(
                                div()
                                    .id("link-field")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .child(
                                        Input::new(&self.link)
                                            .appearance(false)
                                            .disabled(resolving)
                                            .h_full()
                                            .px(px(0.))
                                            .font_family(tokens::FONT_FAMILY_MONO)
                                            .text_size(px(11.5))
                                            .text_color(if ellipsized {
                                                gpui_kit::transparent_black().into()
                                            } else {
                                                tokens::text()
                                            }),
                                    )
                                    .when(ellipsized, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .flex()
                                                .items_center()
                                                .bg(ui::panel())
                                                .cursor_text()
                                                .child(
                                                    ui::mono(11.5, 16.)
                                                        .w_full()
                                                        .truncate()
                                                        .text_color(tokens::text())
                                                        .child(value.clone()),
                                                ),
                                        )
                                    })
                                    .on_click({
                                        let focus = self.link.read(cx).focus_handle(cx);
                                        move |_, window, cx| focus.focus(window, cx)
                                    }),
                            )
                            .when(resolving, |this| {
                                this.child(
                                    h_flex()
                                        .flex_none()
                                        .gap(px(6.))
                                        .text_size(px(11.5))
                                        .line_height(px(16.))
                                        .text_color(tokens::text2())
                                        .child(ui::spinner("link-resolving", 12.))
                                        .child("Resolving\u{2026}"),
                                )
                            }),
                    )
                    .child(if empty || resolving {
                        ui::disabled_button("link-add", "Add")
                            .w(px(64.))
                            .h(px(32.))
                            .px(px(0.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .into_any_element()
                    } else {
                        ui::button("link-add", "Add", Weight::Primary, false)
                            .w(px(64.))
                            .h(px(32.))
                            .px(px(0.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .on_click(cx.listener(|this, _, window, cx| this.add_link(window, cx)))
                            .into_any_element()
                    }),
            )
            .children(error.map(|(line, manage)| {
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .mt(px(6.))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(ui::red())
                    .child(ui::icon("circle-alert", 12.))
                    .child(line)
                    .when(manage, |this| {
                        this.child(
                            div()
                                .id("link-manage")
                                .ml(px(4.))
                                .text_color(ui::accent())
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .child("Manage key")
                                .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                        )
                    })
            }))
    }

    pub(super) fn groups_off_note(
        &self,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .when_some(width, |this, w| this.w(px(w)))
            .items_center()
            .gap(px(12.))
            .py(px(12.))
            .px(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel2())
            .child(div().text_color(ui::accent()).child(ui::icon("users", 16.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        ui::text(12.5, 17.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child("Group experiences are off"),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(tokens::text2())
                            .child("Add")
                            .child(
                                ui::mono(11., 17.)
                                    .text_color(tokens::text())
                                    .child("legacy-group:manage"),
                            )
                            .child("to your key to list the games of groups you manage."),
                    ),
            )
            .child(
                ui::button("groups-manage", "Manage key", Weight::Secondary, true)
                    .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
            )
    }

    pub(super) fn empty_state(
        &self,
        title: &'static str,
        text: String,
        with_link: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap(px(20.))
            .child(
                v_flex()
                    .flex_none()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(64.))
                    .pb(px(8.))
                    .child(
                        h_flex()
                            .size(px(56.))
                            .rounded(px(14.))
                            .bg(ui::panel2())
                            .border_1()
                            .border_color(tokens::border())
                            .items_center()
                            .justify_center()
                            .text_color(tokens::text2())
                            .child(ui::icon("folder", 22.)),
                    )
                    .child(
                        ui::text(16., 22.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(
                        ui::text(12.5, 19.)
                            .max_w(px(480.))
                            .text_center()
                            .text_color(tokens::text2())
                            .child(text),
                    )
                    .when(with_link, |this| {
                        this.child(div().mt(px(6.)).child(self.link_row(cx)))
                    })
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .mt(px(4.))
                            .child(
                                ui::icon_button(
                                    "empty-manage",
                                    "key-round",
                                    "Manage key",
                                    Weight::Secondary,
                                    false,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                            )
                            .child(
                                ui::external_button(
                                    "empty-dashboard",
                                    "Open Creator Dashboard",
                                    Weight::Secondary,
                                    false,
                                )
                                .on_click(|_, _, cx| {
                                    cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL)
                                }),
                            ),
                    ),
            )
            .when(self.groups_off(), |this| {
                this.child(
                    h_flex()
                        .justify_center()
                        .child(self.groups_off_note(Some(640.), cx)),
                )
            })
    }
}
