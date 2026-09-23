//! The detail pane's cards: the PROPERTIES table and the CONTENTS grid.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::shell::argon_sync::{ChangeKind, DiffNode};
use crate::tokens;

use super::super::values::{chip, class_icon, unknown, FormattedProperty, Side};

/// `panel2` on a hairline, radius 6: a 28 px header row, then 36 px rows
/// with a hairline between them, `180 | 1fr | 28 | 1fr` (or `180 | 1fr`).
pub(super) fn properties_table(node: &DiffNode, properties: &[FormattedProperty]) -> Div {
    let update = node.kind == ChangeKind::Updated;
    let head = |text: &'static str| {
        div()
            .text_size(tokens::text_xxs())
            .line_height(tokens::line_xxs())
            .font_weight(tokens::WEIGHT_BOLD)
            .text_color(tokens::text3())
            .child(text)
    };
    let header = h_flex()
        .h(px(28.))
        .px(px(12.))
        .items_center()
        .border_b_1()
        .border_color(tokens::border())
        .child(div().w(px(180.)).flex_none().child(head("PROPERTY")))
        .map(|this| {
            if update {
                this.child(div().flex_1().min_w_0().child(head("BEFORE")))
                    .child(div().w(px(28.)).flex_none())
                    .child(div().flex_1().min_w_0().child(head("AFTER")))
            } else {
                this.child(div().flex_1().min_w_0().child(head("VALUE")))
            }
        });
    let last = properties.len().saturating_sub(1);
    let rows = properties.iter().enumerate().map(|(index, property)| {
        let cell = |side: Side, value: Option<&super::super::values::Cell>| -> AnyElement {
            match value {
                Some(value) => chip(
                    (
                        "argon-diff-chip",
                        index * 2 + if side == Side::Before { 0 } else { 1 },
                    ),
                    value,
                    side,
                ),
                None => unknown(),
            }
        };
        h_flex()
            .h(px(36.))
            .px(px(12.))
            .items_center()
            .when(index < last, |this| {
                this.border_b_1().border_color(tokens::border())
            })
            .child(
                div()
                    .w(px(180.))
                    .flex_none()
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .text_color(tokens::text2())
                    .child(property.name.clone()),
            )
            .map(|this| {
                if update {
                    this.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .child(cell(Side::Before, property.before.as_ref())),
                    )
                    .child(
                        div()
                            .w(px(28.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .text_color(tokens::text3())
                            .child(Icon::new(IconName::ArrowRight).size(px(12.))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .child(cell(Side::After, property.after.as_ref())),
                    )
                } else {
                    this.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .child(cell(Side::After, property.after.as_ref())),
                    )
                }
            })
    });
    v_flex()
        .flex_none()
        .rounded(tokens::RADIUS_TILE)
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .child(header)
        .children(rows)
}

/// Three columns of 36 px tiles, 8 apart: icon 14, class in `text2`,
/// count in mono `text`.
pub(super) fn contents_grid(node: &DiffNode, pack: IconPack) -> Div {
    let mut tiles = node
        .contents
        .iter()
        .map(|(class, count)| {
            h_flex()
                .flex_1()
                .min_w_0()
                .h(px(36.))
                .px(px(12.))
                .gap(px(8.))
                .items_center()
                .rounded(tokens::RADIUS_TILE)
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::border())
                .child(class_icon(class, pack, 14.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(tokens::text_md())
                        .line_height(tokens::line_md())
                        .text_color(tokens::text2())
                        .child(class.clone()),
                )
                .child(
                    div()
                        .font_family(tokens::FONT_FAMILY_MONO)
                        .text_size(tokens::text_badge())
                        .line_height(tokens::line_badge())
                        .text_color(tokens::text())
                        .child(count.to_string()),
                )
                .into_any_element()
        })
        .peekable();
    let mut grid = v_flex().flex_none().gap(px(8.));
    while tiles.peek().is_some() {
        let mut row = h_flex().gap(px(8.));
        for _ in 0..3 {
            row = row.child(match tiles.next() {
                Some(tile) => div().flex_1().min_w_0().child(tile),
                None => div().flex_1().min_w_0(),
            });
        }
        grid = grid.child(row);
    }
    grid
}
