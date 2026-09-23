//! The window's top and bottom bands: the title tile, the project line,
//! the filter and the search above the body; the note, Cancel and Accept
//! below it.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::model::Filter;
use super::ArgonDiffWindow;

impl ArgonDiffWindow {
    /// 61 tall with its hairline: `padding:12px 16px` (14 when narrow),
    /// `gap:12px`.
    pub(super) fn header(
        &mut self,
        project: &str,
        counts: [usize; 4],
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let total = counts[0];
        let subtitle = if narrow {
            format!("{project} \u{b7} {total} changes")
        } else {
            format!("{project} \u{b7} {total} changes from the server")
        };
        let segments = Filter::ALL.iter().zip(counts).map(|(&filter, count)| {
            let active = filter == self.filter;
            let enabled = count > 0;
            h_flex()
                .id(SharedString::from(format!(
                    "argon-diff-filter-{}",
                    filter.label()
                )))
                .h(px(22.))
                .px(px(10.))
                .gap(px(5.))
                .items_center()
                .rounded(tokens::RADIUS_SEGMENT)
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .map(|this| {
                    if active {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else if enabled {
                        this.tab_index(0)
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| this.bg(tokens::hover()))
                            .focus_visible(|this| {
                                this.shadow(tokens::focus_ring(tokens::field_select()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter = filter;
                                this.ensure_selection(cx);
                                cx.notify();
                            }))
                    } else {
                        this.text_color(tokens::text3())
                    }
                })
                .child(filter.label())
                .child(
                    div()
                        .font_family(tokens::FONT_FAMILY_MONO)
                        .text_size(px(10.))
                        .line_height(tokens::line_xs())
                        .font_weight(if active {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if active {
                            tokens::check_on()
                        } else {
                            tokens::text3()
                        })
                        .child(count.to_string()),
                )
        });
        let search_handle = self.search.read(cx).focus_handle(cx);
        h_flex()
            .h(px(61.))
            .flex_none()
            .items_center()
            .gap(px(12.))
            .px(px(if narrow { 14. } else { 16. }))
            .py(px(12.))
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::RADIUS_TILE)
                    .bg(tokens::accent_soft())
                    .text_color(tokens::check_on())
                    .child(Icon::new(IconName::GitBranch).size(px(16.))),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .ml(px(-2.))
                    .child(
                        div()
                            .text_size(tokens::text_lg())
                            .line_height(tokens::line_lg())
                            .font_weight(tokens::WEIGHT_BOLD)
                            .text_color(tokens::text())
                            .child("Review changes"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text2())
                            .child(subtitle),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .p(px(2.))
                    .gap(px(2.))
                    .rounded(tokens::RADIUS)
                    .bg(tokens::field_select())
                    .children(segments),
            )
            .when(!narrow, |this| {
                this.child(
                    h_flex()
                        .track_focus(&search_handle)
                        .focus(|this| this.border_color(tokens::accent_line()))
                        .flex_none()
                        .w(px(200.))
                        .h(px(28.))
                        .px(px(9.))
                        .gap(px(7.))
                        .items_center()
                        .rounded(tokens::RADIUS)
                        .bg(tokens::field_select())
                        .border_1()
                        .border_color(tokens::border())
                        .text_color(tokens::text3())
                        .child(Icon::new(IconName::Search).size(px(12.)))
                        .child(
                            div().flex_1().min_w_0().h_full().child(
                                Input::new(&self.search)
                                    .appearance(false)
                                    .h_full()
                                    .px(px(0.))
                                    .text_size(tokens::text_sm())
                                    .text_color(tokens::text()),
                            ),
                        ),
                )
            })
    }

    /// 57 tall with its hairline: the note at the left (wide only), then
    /// Cancel and Accept, 96×34, the dock's own handlers.
    pub(super) fn footer(&mut self, narrow: bool, cx: &mut Context<Self>) -> Div {
        let shell = self.shell.clone();
        let cancel = shell.clone();
        h_flex()
            .h(px(57.))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(if narrow { 14. } else { 16. }))
            .border_t_1()
            .border_color(tokens::border())
            .when(!narrow, |this| {
                this.child(
                    div()
                        .text_size(tokens::text_sm())
                        .line_height(tokens::line_sm())
                        .text_color(tokens::text3())
                        .child("Nothing is applied until you accept."),
                )
            })
            .child(div().flex_1())
            .child(
                button("argon-diff-cancel", "Cancel", false).on_click(cx.listener(
                    move |_, _, _, cx| {
                        cancel.update(cx, |shell, cx| shell.argon_cancel_pending(cx));
                    },
                )),
            )
            .child(
                button("argon-diff-accept", "Accept", true).on_click(cx.listener(
                    move |_, _, _, cx| {
                        shell.update(cx, |shell, cx| shell.argon_accept_pending(cx));
                    },
                )),
            )
    }
}

/// 96×34, radius 5, 12.5/17: `accent` with a `bg` label at 700, or
/// `panel2` on `border2` with a `text` label at 600.
fn button(id: &'static str, label: &'static str, primary: bool) -> Stateful<Div> {
    h_flex()
        .id(id)
        .tab_index(0)
        .flex_none()
        .w(px(96.))
        .h(px(34.))
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_action())
        .line_height(tokens::line_action())
        .cursor_pointer()
        .map(|this| {
            if primary {
                this.bg(tokens::check_on())
                    .text_color(tokens::black())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .hover(|this| this.bg(tokens::accent_hover()))
            } else {
                this.bg(tokens::field_select())
                    .border_1()
                    .border_color(tokens::border2())
                    .text_color(tokens::text())
                    .font_weight(tokens::WEIGHT_SEMIBOLD)
                    .hover(|this| this.bg(tokens::secondary_hover()))
            }
        })
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(label)
}
