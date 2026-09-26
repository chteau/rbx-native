//! A page as the window shows it: the header with its reset button, the
//! sections, and the search field that replaces it with results.

use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::kit::{self, text};
use super::nav::Page;
use super::SettingsWindow;

impl SettingsWindow {
    /// What a page shows that isn't a section of rows.
    pub(super) fn body(&self) -> Option<AnyElement> {
        (self.page == Page::Beta).then(|| self.beta_body())
    }

    pub(super) fn page_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sections = self.sections(window, cx);
        let body = self.body();
        let resets: Vec<_> = sections
            .iter()
            .flat_map(|section| {
                section
                    .rows
                    .iter()
                    .filter_map(|row| row.reset.clone())
                    .chain(section.resets.iter().cloned())
            })
            .collect();
        let shell = self.shell.clone();
        let reset_page = kit::header_button(
            "reset-page",
            "Reset page",
            (!resets.is_empty()).then_some(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| {
                    for reset in &resets {
                        reset(shell, cx);
                    }
                })
            }),
        );
        let header_button = match self.page {
            Page::Argon => Some(self.argon_header(cx)),
            page if page.has_reset() => Some(reset_page),
            _ => None,
        };
        let lead = (self.page == Page::Argon).then(|| self.argon_scope_bar(cx));
        let (title, subtitle) = self.page.heading();
        let shell = self.shell.clone();
        v_flex()
            .id("settings-page")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.page_scroll)
            .vertical_scrollbar(&self.page_scroll)
            .child(
                v_flex()
                    .pt(px(26.))
                    .pr(px(40.))
                    .pb(px(40.))
                    .pl(px(36.))
                    .gap(px(22.))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(16.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(4.))
                                    .child(
                                        text(20., 26.).font_weight(FontWeight::BOLD).child(title),
                                    )
                                    .child(
                                        text(12.5, 18.).text_color(tokens::text2()).child(subtitle),
                                    ),
                            )
                            .children(header_button),
                    )
                    .children(lead)
                    .children(
                        sections
                            .into_iter()
                            .enumerate()
                            .map(|(i, section)| kit::section(i, section, &shell)),
                    )
                    .children(body),
            )
    }

    pub(super) fn search_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let typed = !self.query(cx).is_empty();
        h_flex()
            .h(px(32.))
            .flex_none()
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(8.))
            .items_center()
            .border_1()
            .border_color(if typed {
                tokens::accent_line()
            } else {
                tokens::border()
            })
            .rounded(px(6.))
            .bg(tokens::field_select())
            .text_color(tokens::text3())
            .child(kit::icon("search", 14.))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .appearance(false)
                        .px_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(tokens::text()),
                ),
            )
            .map(|this| {
                if typed {
                    this.child(
                        h_flex()
                            .id("search-clear")
                            .flex_none()
                            .size(px(18.))
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(rgba(0xFFFFFF0F))
                            .text_color(tokens::text2())
                            .cursor_pointer()
                            .child(kit::icon("x", 10.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.search
                                    .update(cx, |state, cx| state.set_value("", window, cx));
                                cx.notify();
                            })),
                    )
                } else {
                    this.child(kit::key_hint("Ctrl F"))
                }
            })
            .into_any_element()
    }
}
