//! Home's sidebar: brand, New place, Open file, the page links, and the
//! key card at the bottom.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::*;
use crate::launcher::home_window::{HomeWindow, KeyState, Page};
use crate::launcher::ui::{self};
use crate::tokens;

impl HomeWindow {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = |id: &'static str,
                   glyph: &'static str,
                   label: &'static str,
                   page: Page,
                   cx: &mut Context<Self>| {
            let active = self.page == page;
            h_flex()
                .id(id)
                .h(px(36.))
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .rounded(px(6.))
                .text_size(px(12.5))
                .line_height(px(17.))
                .cursor_pointer()
                .map(|this| {
                    if active {
                        this.bg(tokens::accent_soft())
                            .text_color(ui::accent())
                            .font_weight(FontWeight::SEMIBOLD)
                    } else {
                        this.text_color(tokens::text2()).hover(|this| {
                            this.bg(tokens::hover_subtle()).text_color(tokens::text())
                        })
                    }
                })
                .child(ui::icon(glyph, 16.))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
        };
        v_flex()
            .w(px(SIDEBAR))
            .flex_none()
            .gap(px(4.))
            .pt(px(18.))
            .px(px(12.))
            .pb(px(12.))
            .bg(ui::bg())
            .border_r_1()
            .border_color(tokens::border())
            .child(
                h_flex()
                    .h(px(32.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .mb(px(10.))
                    .child(
                        div()
                            .size(px(22.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(ui::accent()),
                    )
                    .child(
                        ui::text(14., 20.)
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text())
                            .child("RbxNative"),
                    ),
            )
            .child(
                h_flex()
                    .id("home-new-place")
                    .h(px(36.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens::text())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover_subtle()))
                    .child(
                        h_flex()
                            .size(px(20.))
                            .flex_none()
                            .rounded_full()
                            .bg(ui::accent())
                            .text_color(ui::bg())
                            .items_center()
                            .justify_center()
                            .child(ui::icon("plus", 13.)),
                    )
                    .child("New place")
                    .on_click(cx.listener(|this, _, _, cx| this.new_place(cx))),
            )
            .child(
                h_flex()
                    .id("home-open-file")
                    .h(px(36.))
                    .mb(px(6.))
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(tokens::text2())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover_subtle()).text_color(tokens::text()))
                    .child(ui::icon("folder-open", 16.))
                    .child("Open file\u{2026}")
                    .on_click(cx.listener(|this, _, _, cx| this.open_file(cx))),
            )
            .child(nav("nav-home", "house", "Home", Page::Home, cx))
            .child(nav("nav-recent", "clock", "Recent", Page::Recent, cx))
            .child(nav("nav-games", "gamepad-2", "My Games", Page::MyGames, cx))
            .child(div().flex_1())
            .child(self.account_card(cx))
    }

    pub(super) fn account_card(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.has_key() {
            return v_flex()
                .gap(px(6.))
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(tokens::border())
                .bg(ui::panel())
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("key-round", 14.).text_color(tokens::text2()))
                        .child(
                            ui::text(12., 16.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(tokens::text())
                                .child("No API key"),
                        ),
                )
                .child(
                    ui::text(11., 15.).text_color(tokens::text2()).child(
                        "Local files work. Opening and publishing Roblox places needs a key.",
                    ),
                )
                .child(
                    ui::text(11.5, 16.)
                        .id("sidebar-set-up")
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::accent())
                        .cursor_pointer()
                        .hover(|this| this.text_color(tokens::text()))
                        .child("Set up a key")
                        .on_click(cx.listener(|this, _, window, cx| this.set_up_key(window, cx))),
                )
                .into_any_element();
        }
        let (name, status, dot) = match &self.key {
            KeyState::Ready { owner, report } if report.ready() => {
                (owner.clone(), "Key ready", ui::green())
            }
            KeyState::Ready { owner, .. } => (owner.clone(), "Key needs attention", ui::red()),
            KeyState::Checking => ("Your key".to_string(), "Checking\u{2026}", tokens::text3()),
            _ => (
                "Your key".to_string(),
                "Couldn\u{2019}t check",
                tokens::text3(),
            ),
        };
        let initial = name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        h_flex()
            .id("sidebar-account")
            .items_center()
            .gap(px(10.))
            .py(px(10.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(tokens::border())
            .bg(ui::panel())
            .cursor_pointer()
            .hover(|this| this.bg(tokens::hover_subtle()))
            .child(
                h_flex()
                    .size(px(28.))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(0x2C3050))
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(ui::accent())
                    .child(initial),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        ui::text(12., 16.)
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::text())
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(tokens::text2())
                            .child(div().size(px(6.)).rounded_full().bg(dot))
                            .child(status),
                    ),
            )
            .child(ui::icon("chevron-right", 14.).text_color(tokens::text3()))
            .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx)))
            .into_any_element()
    }
}
