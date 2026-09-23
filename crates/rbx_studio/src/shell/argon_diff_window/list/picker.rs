//! Narrow: the picker bar that stands in for the list pane — previous
//! and next, the select showing the current change, the position.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::shell::argon_sync::{ChangeKind, DiffNode};
use crate::tokens;

use super::super::model::{self, Row};
use super::super::values::{class_icon, kind_colour};
use super::super::ArgonDiffWindow;

impl ArgonDiffWindow {
    /// Narrow: h45 with its hairline, `padding:0 14px`, `gap:8px`: prev,
    /// the select (marker, icon, name, path, chevron), next, `n / N`.
    pub(in crate::shell::argon_diff_window) fn picker_bar(
        &mut self,
        nodes: &[DiffNode],
        rows: &[Row],
        pack: IconPack,
        cx: &mut Context<Self>,
    ) -> Div {
        let selectable: std::rc::Rc<Vec<usize>> =
            std::rc::Rc::new(rows.iter().filter_map(Row::node_id).collect());
        let position = self
            .selected
            .and_then(|id| selectable.iter().position(|&row| row == id));
        let total = selectable.len();
        let current = self.selected.and_then(|id| model::find(nodes, id));
        let step = |delta: isize| {
            let selectable = selectable.clone();
            move |this: &mut Self, cx: &mut Context<Self>| {
                if let Some(index) = position {
                    let next = index as isize + delta;
                    if next >= 0 && (next as usize) < total {
                        this.select(selectable[next as usize], cx);
                    }
                }
            }
        };
        let (back, forward) = (step(-1), step(1));
        let prev_enabled = position.is_some_and(|index| index > 0);
        let next_enabled = position.is_some_and(|index| index + 1 < total);
        let select = h_flex()
            .id("argon-diff-picker")
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(9.))
            .gap(px(8.))
            .items_center()
            .rounded(tokens::RADIUS)
            .bg(tokens::dock())
            .border_1()
            .border_color(tokens::border2())
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.picker_open = !this.picker_open;
                cx.notify();
            }))
            .when_some(current, |this, node| {
                this.child(
                    div()
                        .flex_none()
                        .w(px(9.))
                        .text_center()
                        .font_family(tokens::FONT_FAMILY_MONO)
                        .text_size(tokens::text_md())
                        .line_height(tokens::line_md())
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .text_color(kind_colour(node.kind))
                        .child(match node.kind {
                            ChangeKind::Added => "+",
                            ChangeKind::Updated => "~",
                            ChangeKind::Removed => "\u{2212}",
                        }),
                )
                .child(class_icon(&node.class, pack, 14.))
                .child(
                    div()
                        .flex_none()
                        .text_size(tokens::text_md())
                        .line_height(tokens::line_md())
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .text_color(tokens::text())
                        .child(node.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(tokens::text_xs())
                        .line_height(tokens::line_xs())
                        .text_color(tokens::text3())
                        .child(node.path.clone()),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::text3())
                    .child(Icon::new(IconName::ChevronDown).size(px(10.))),
            );
        h_flex()
            .h(px(45.))
            .flex_none()
            .px(px(14.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(
                arrow("argon-diff-prev", IconName::ChevronLeft, prev_enabled)
                    .on_click(cx.listener(move |this, _, _, cx| back(this, cx))),
            )
            .child(select)
            .child(
                arrow("argon-diff-next", IconName::ChevronRight, next_enabled)
                    .on_click(cx.listener(move |this, _, _, cx| forward(this, cx))),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::text3())
                    .child(format!(
                        "{} / {total}",
                        position.map_or(0, |index| index + 1)
                    )),
            )
    }
}

/// 28×28, `panel2` on `border2`, a 12 px chevron in `text2`; at either
/// end `text3` on `border`, inert.
fn arrow(id: &'static str, icon: IconName, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::RADIUS)
        .bg(tokens::field_select())
        .border_1()
        .map(|this| {
            if enabled {
                this.border_color(tokens::border2())
                    .text_color(tokens::text2())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::secondary_hover()))
            } else {
                this.border_color(tokens::border())
                    .text_color(tokens::text3())
            }
        })
        .child(Icon::new(icon).size(px(12.)))
}
