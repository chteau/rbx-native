//! The window's drawing: the tab sidebar, the page of rows grouped by
//! where they live, and the Save/Cancel footer.

mod rows;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};

use super::super::chrome;
use super::super::settings_window::kit;
use super::catalog::{Tab, UNREACHABLE};
use super::{tokens, GameSettingsWindow};

impl Render for GameSettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.reseed {
            self.reseed_inputs(window, cx);
        }
        v_flex()
            .id("game-settings-window")
            .track_focus(&self.focus)
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_size(px(13.))
            .text_color(tokens::text())
            .child(chrome::window_topbar(
                "Game Settings".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(self.page(cx)),
            )
    }
}

fn id(prefix: &str, n: impl std::fmt::Display) -> SharedString {
    SharedString::from(format!("{prefix}-{n}"))
}

impl GameSettingsWindow {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(220.))
            .flex_none()
            .h_full()
            .bg(tokens::black())
            .border_r_1()
            .border_color(tokens::border())
            .p(px(10.))
            .gap(px(2.))
            .children(Tab::ALL.iter().enumerate().map(|(i, &tab)| {
                let selected = tab == self.tab;
                h_flex()
                    .id(id("gs-tab", i))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(8.))
                    .rounded(px(5.))
                    .items_center()
                    .cursor_pointer()
                    .text_color(if selected {
                        tokens::check_on()
                    } else {
                        tokens::text2()
                    })
                    .when(selected, |d| d.bg(tokens::accent_soft()))
                    .when(!selected, |d| d.hover(|h| h.bg(tokens::hover())))
                    .child(kit::icon(tab.glyph(), 14.))
                    .child(kit::text(12.5, 18.).child(tab.title()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
            }))
    }

    fn page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let cloud_rows = self.cloud_rows(cx);
        let local_rows = self.local_rows(cx);
        let missed: Vec<AnyElement> = UNREACHABLE
            .iter()
            .filter(|u| u.tab == tab)
            .map(|u| {
                v_flex()
                    .px(px(14.))
                    .py(px(10.))
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(tokens::border())
                    .text_color(tokens::text3())
                    .child(kit::text(12.5, 18.).child(u.label))
                    .child(kit::text(11.5, 16.).child(u.reason))
                    .into_any_element()
            })
            .collect();
        let body = v_flex()
            .id("game-settings-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(24.))
            .gap(px(20.))
            .child(
                v_flex()
                    .gap(px(4.))
                    .child(
                        kit::text(20., 26.)
                            .font_weight(FontWeight::BOLD)
                            .child(tab.title()),
                    )
                    .child(
                        kit::text(12.5, 18.)
                            .text_color(tokens::text2())
                            .child(tab.subtitle()),
                    ),
            )
            .when(!cloud_rows.is_empty(), |d| {
                d.child(section(
                    "On Roblox",
                    Some("Roblox"),
                    "Saved to the live experience through Open Cloud.",
                    cloud_rows,
                ))
            })
            .when(!local_rows.is_empty(), |d| {
                d.child(section(
                    "In this place file",
                    Some("Place file"),
                    "Saved with this file; Roblox sees it when the place is published.",
                    local_rows,
                ))
            })
            .when(!missed.is_empty(), |d| {
                d.child(section("Not reachable from here", None, "", missed))
            });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(div().flex_1().min_h_0().child(body))
            .child(self.footer(cx))
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_save = self.is_dirty() && !self.saving && self.invalid.is_empty();
        let save = if can_save {
            ui::button("gs-save", "Save", Weight::Primary, false)
                .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
                .into_any_element()
        } else {
            ui::disabled_button("gs-save-off", "Save").into_any_element()
        };
        let scopes = self.cloud.scopes();
        h_flex()
            .flex_none()
            .px(px(24.))
            .py(px(12.))
            .gap(px(10.))
            .items_center()
            .border_t_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .children(self.notice.as_ref().map(|n| {
                        kit::text(12., 16.)
                            .text_color(if n.ok { ui::green() } else { ui::red() })
                            .child(n.text.clone())
                    })),
            )
            .when(!scopes.is_empty(), |d| {
                d.child(
                    kit::mono(10.5, 14.)
                        .flex_none()
                        .text_color(tokens::text3())
                        .child(format!("Key needs {}", scopes.join(", "))),
                )
            })
            .when(self.saving, |d| d.child(ui::spinner("gs-saving", 16.)))
            .child(
                ui::button("gs-cancel", "Cancel", Weight::Secondary, false)
                    .on_click(cx.listener(|this, _, _, cx| this.revert(cx))),
            )
            .child(save)
    }
}

fn section(
    title: &'static str,
    badge: Option<&'static str>,
    line: &'static str,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    kit::text(10.5, 14.)
                        .font_weight(FontWeight::BOLD)
                        .text_color(tokens::text3())
                        .child(title.to_uppercase()),
                )
                .children(badge.map(|b| ui::tag(b, ui::accent(), ui::wash()))),
        )
        .when(!line.is_empty(), |d| {
            d.child(kit::text(11.5, 16.).text_color(tokens::text2()).child(line))
        })
        .child(
            v_flex()
                .rounded(px(8.))
                .border_1()
                .border_color(tokens::border())
                .bg(tokens::black())
                .children(rows),
        )
}
