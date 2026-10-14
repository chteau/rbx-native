//! The Output dock's strip controls: the run count, the search box, the
//! level segments, the context select and Clear.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::chrome;
use super::super::menu::{self, MenuId};
use super::{ContextFilter, OutputFilter, Shell, SEARCH_MIN_WIDTH};

impl Shell {
    /// The Output dock's strip controls (see `shell::workspace`): the run
    /// count, then — at the strip's far end — the search box, the level
    /// filter and the Clear button.
    pub(in crate::shell) fn output_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.output_filter;
        h_flex()
            .flex_1()
            .h_full()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .ml(px(16.))
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::text_placeholder())
                    .child(format!("{} runs", self.output.len())),
            )
            // No subscription behind it: the value is read straight off the
            // `InputState` at render, the same way the Properties panel's
            // own filter box is (`shell::panels`). Takes all the width the
            // strip has left, as the reference's own search does.
            .child(div().flex_1().min_w(px(SEARCH_MIN_WIDTH)).child(
                super::super::workspace::search_field_compact(
                    &self.tab_order,
                    &self.output_search,
                    cx,
                ),
            ))
            .child(
                // One segmented control, not four separate buttons: the
                // reference draws the four levels as segments of a single
                // `field_select` pill, the active one carrying the accent.
                h_flex()
                    .flex_none()
                    .bg(tokens::field_select())
                    .rounded(tokens::radius())
                    .p(px(2.))
                    .gap(px(2.))
                    .children(OutputFilter::ALL.map(|level| {
                        let selected = current == level;
                        h_flex()
                            .id(("output-filter", level as usize))
                            .tab_index(self.tab_order.next())
                            .items_center()
                            .px(px(9.))
                            .py(px(3.))
                            .rounded(tokens::radius_segment())
                            .text_size(tokens::text_xs())
                            .line_height(tokens::line_xs())
                            .cursor_pointer()
                            .map(|this| {
                                if selected {
                                    this.bg(tokens::accent_soft())
                                        .text_color(tokens::check_on())
                                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                                } else {
                                    this.text_color(tokens::text_placeholder())
                                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                                }
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                shell.output_filter = level;
                                cx.notify();
                            }))
                            .child(level.label())
                    })),
            )
            .child(self.output_context_picker(cx))
            .child(
                h_flex()
                    .id("output-clear")
                    .tab_index(self.tab_order.next())
                    .flex_none()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(tokens::radius())
                    .cursor_pointer()
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::text_placeholder())
                    .hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text_strong())
                    })
                    .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.output.clear();
                        cx.notify();
                    }))
                    .child("Clear"),
            )
    }

    /// Studio's context filter as a compact select beside the level
    /// segments: All Contexts, Edit, Server, Client.
    fn output_context_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.output_context;
        let items = ContextFilter::ALL
            .into_iter()
            .map(|context| {
                menu::item(context.label())
                    .checked(context == current)
                    .on_click(move |shell, cx| {
                        shell.output_context = context;
                        cx.notify();
                    })
            })
            .collect();
        let trigger = h_flex()
            .id("output-context")
            .tab_index(self.tab_order.next())
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(9.))
            .py(px(5.))
            .rounded(tokens::radius())
            .bg(tokens::field_select())
            .text_size(tokens::text_xs())
            .line_height(tokens::line_xs())
            .text_color(if current == ContextFilter::All {
                tokens::text_placeholder()
            } else {
                tokens::check_on()
            })
            .cursor_pointer()
            .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
            .child(current.label())
            .child(Icon::new(IconName::ChevronDown).size(px(10.)));
        menu::dropdown(
            self,
            MenuId::OutputContext,
            chrome::Trigger::new(trigger),
            items,
            cx,
        )
    }
}
