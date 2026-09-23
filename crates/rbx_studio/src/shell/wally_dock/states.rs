//! What stands in for the cards: the skeletons while Featured loads,
//! the error state with Try again, and the all-up-to-date state.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;

use crate::tokens;

use super::super::Shell;

/// Skeleton bars while Featured loads: the title bar's width, then the
/// two description bars' widths as fractions of the card.
pub(super) const SKELETONS: [(f32, f32, f32); 8] = [
    (120., 0.90, 0.60),
    (96., 0.80, 0.45),
    (132., 0.95, 0.55),
    (84., 0.70, 0.40),
    (110., 0.85, 0.50),
    (128., 0.92, 0.62),
    (90., 0.75, 0.48),
    (116., 0.88, 0.58),
];

impl Shell {
    /// The registry couldn't be reached: a cloud-off glyph, two lines,
    /// and "Try again", which re-runs whatever failed.
    pub(super) fn error_state(&mut self, cx: &mut Context<Self>) -> Div {
        empty_state(
            IconName::CloudOff,
            "Couldn't reach the Wally registry",
            "Check your connection, then try again.".to_owned(),
        )
        .child(div().h(px(6.)))
        .child(
            h_flex()
                .id("wally-retry")
                .tab_index(self.tab_order.next())
                .h(px(28.))
                .px(px(14.))
                .items_center()
                .rounded(tokens::RADIUS)
                .bg(tokens::field_select())
                .border_1()
                .border_color(tokens::border2())
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::text())
                .cursor_pointer()
                .hover(|this| this.bg(tokens::secondary_hover()))
                .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                .on_click(cx.listener(|shell, _, _, cx| shell.wally_retry(cx)))
                .child("Try again"),
        )
    }
}

/// A centred stack: a 20px glyph in a 28px box, a title, a line under it.
pub(super) fn empty_state(icon: IconName, title: &'static str, detail: String) -> Div {
    v_flex()
        .flex_1()
        .min_h_0()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens::text3())
                .child(Icon::new(icon).size(px(20.))),
        )
        .child(
            div()
                .text_size(tokens::text_md())
                .line_height(tokens::line_md())
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::text())
                .child(title),
        )
        .child(
            div()
                .text_size(tokens::text_sm())
                .line_height(tokens::line_sm())
                .text_color(tokens::text2())
                .child(detail),
        )
}

/// A 74px placeholder card: a title bar and a version bar, then two
/// description bars, 10px apart, no motion.
pub(super) fn skeleton((title, first, second): (f32, f32, f32)) -> AnyElement {
    let bar = |width: Length, height: f32, color: Rgba| {
        div().w(width).h(px(height)).rounded(px(3.)).bg(color)
    };
    v_flex()
        .w_full()
        .min_w_0()
        .h(px(74.))
        .gap(px(10.))
        .p(px(12.))
        .rounded(tokens::RADIUS_TILE)
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(bar(px(title).into(), 10., tokens::border2()))
                .child(bar(px(36.).into(), 8., tokens::border())),
        )
        .child(bar(relative(first).into(), 8., tokens::border()))
        .child(bar(relative(second).into(), 8., tokens::border()))
        .into_any_element()
}
