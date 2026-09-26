//! The search field and the three pages, as a 208px rail (wide) or a top
//! band with a segmented control (narrow).

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::wally_sync::Page;
use super::super::Shell;
use super::{PAGE_PADDING, RAIL_WIDTH};

/// The rail's two counts: Installed always shows its number; Updates
/// shows a pill only when there is at least one.
#[derive(Debug, Clone, Copy)]
pub(super) struct Counts {
    pub(super) installed: usize,
    pub(super) updates: usize,
}

const PAGES: [(Page, &str, IconName); 3] = [
    (Page::Home, "Home", IconName::House),
    (Page::Installed, "Installed", IconName::Package),
    (Page::Updates, "Updates", IconName::CircleArrowUp),
];

impl Shell {
    /// The rail row (or band segment) drawn as current: the page, except
    /// while a search is on screen, when none is — a result isn't Home.
    fn wally_current_row(&self) -> Option<Page> {
        (self.wally.query.is_empty()).then_some(self.wally_page())
    }

    /// Wide: 14/12 padding, the field, then the page rows 2px apart.
    pub(super) fn rail(&mut self, counts: Counts, cx: &mut Context<Self>) -> Div {
        let current = self.wally_current_row();
        let rows = PAGES.map(|(page, label, icon)| {
            let selected = Some(page) == current;
            h_flex()
                .id(SharedString::from(format!("wally-page-{label}")))
                .h(px(28.))
                .px(px(9.))
                .gap(px(9.))
                .items_center()
                .rounded(tokens::radius())
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .map(|this| {
                    if selected {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else {
                        this.tab_index(self.tab_order.next())
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| {
                                let this = tokens::hover_fx(this);
                                this.bg(tokens::hover_subtle()).text_color(tokens::text())
                            })
                            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.wally_set_page(page, cx);
                            }))
                    }
                })
                .child(Icon::new(icon).size(px(14.)))
                .child(label)
                .children(count_badge(page, counts, px(6.)).map(|badge| badge.ml_auto()))
        });
        v_flex()
            .w(px(RAIL_WIDTH))
            .flex_none()
            .py(px(PAGE_PADDING))
            .px(px(12.))
            .gap(px(10.))
            .child(self.search_field(cx))
            .child(v_flex().gap(px(2.)).children(rows))
    }

    /// Narrow: 14px padding at top and sides, the field, 8px, then a
    /// 28px segmented control with the three pages.
    pub(super) fn band(&mut self, counts: Counts, cx: &mut Context<Self>) -> Div {
        let current = self.wally_current_row();
        let segments = PAGES.map(|(page, label, _)| {
            let selected = Some(page) == current;
            h_flex()
                .id(SharedString::from(format!("wally-page-{label}")))
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .px(px(8.))
                .gap(px(6.))
                .items_center()
                .justify_center()
                .rounded(tokens::radius_segment())
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .map(|this| {
                    if selected {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else {
                        this.tab_index(self.tab_order.next())
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                            .focus_visible(|this| {
                                this.shadow(tokens::focus_ring(tokens::field_select()))
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.wally_set_page(page, cx);
                            }))
                    }
                })
                .child(label)
                .children(count_badge(page, counts, px(5.)))
        });
        v_flex()
            .flex_none()
            .pt(px(PAGE_PADDING))
            .px(px(PAGE_PADDING))
            .gap(px(8.))
            .child(self.search_field(cx))
            .child(
                h_flex()
                    .h(px(28.))
                    .p(px(2.))
                    .gap(px(2.))
                    .rounded(tokens::radius())
                    .bg(tokens::field_select())
                    .children(segments),
            )
    }

    /// 30px, `panel2` on a `border` hairline: a 13px search glyph and the
    /// query, `accent_line` while focused.
    fn search_field(&mut self, cx: &mut Context<Self>) -> Div {
        let state = self.wally_query.clone();
        let has_query = !self.wally.query.is_empty();
        let handle = state.read(cx).focus_handle(cx);
        self.tab_order.register(&handle);
        h_flex()
            .track_focus(&handle)
            .focus(|this| this.border_color(tokens::accent_line()))
            .h(px(30.))
            .flex_none()
            .pl(px(10.))
            .pr(px(6.))
            .gap(px(8.))
            .items_center()
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .text_color(tokens::text3())
            .child(Icon::new(IconName::Search).size(px(13.)))
            .child(
                div().flex_1().min_w_0().h_full().child(
                    Input::new(&state)
                        .appearance(false)
                        .h_full()
                        .px(px(0.))
                        .text_size(tokens::text_md())
                        .text_color(tokens::text()),
                ),
            )
            .children(has_query.then(|| {
                // 20×20 hit area, a 12px ×; clearing the query brings Home back.
                div()
                    .id("wally-clear")
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::radius_badge())
                    .cursor_pointer()
                    .text_color(tokens::text3())
                    .hover(|this| tokens::hover_fx(this).text_color(tokens::text()))
                    .tooltip(|window, cx| super::super::tooltip::text("Clear search", window, cx))
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.wally_search_for(String::new(), window, cx);
                    }))
                    .child(Icon::new(IconName::X).size(px(12.)))
            }))
    }
}

/// Installed's count in mono `text3`; Updates' pill in `accent_soft`,
/// or nothing at zero. `pill_padding` is the pill's horizontal padding:
/// 6 in the rail, 5 in the band.
fn count_badge(page: Page, counts: Counts, pill_padding: Pixels) -> Option<Div> {
    match page {
        Page::Home => None,
        Page::Installed => Some(
            div()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .font_weight(FontWeight::NORMAL)
                .text_color(tokens::text3())
                .child(counts.installed.to_string()),
        ),
        Page::Updates => (counts.updates > 0).then(|| {
            div()
                .px(pill_padding)
                .rounded(tokens::radius_badge())
                .bg(tokens::accent_soft())
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::check_on())
                .child(counts.updates.to_string())
        }),
    }
}
