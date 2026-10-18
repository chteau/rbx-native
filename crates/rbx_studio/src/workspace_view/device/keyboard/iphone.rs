//! The iPhone's key pages, and the strip below iOS keyboards: the globe and
//! dictation under an iPhone's portrait keys, the home indicator on every
//! iOS one.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::super::WorkspaceView;
use super::{cap_key, hex, icon, layout_switch, rows, Cap, Ctx, Metrics, Page, Style};

/// Shift's, delete's and the page keys' width, in letter keys.
const SIDE: f32 = 1.3;
/// Return's width, in letter keys.
const RETURN: f32 = 2.75;
/// The globe's and dictation's size.
const STRIP_ICON: f32 = 28.0;

fn row(m: &Metrics) -> Div {
    div()
        .relative()
        .flex()
        .flex_row()
        .flex_none()
        .w_full()
        .h(m.px(m.key_h))
        .px(m.px(m.margin))
        .gap(m.px(m.key_gap))
}

/// The width each of the five wide middle keys gets on the 123 and #+=
/// pages, framed by the same side keys as the letters' seven.
fn wide_key_w(m: &Metrics) -> f32 {
    let side = m.key_w * SIDE;
    let inner = m.width - 2.0 * m.margin - 2.0 * side;
    let letters_gap = (inner - 7.0 * m.key_w - 6.0 * m.key_gap) / 2.0;
    (inner - 2.0 * letters_gap - 4.0 * m.key_gap) / 5.0
}

pub(super) fn page(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let side = m.key_w * SIDE;
    let letters = matches!(ctx.page, Page::Letters { .. });
    let keys = match ctx.page {
        Page::Letters { .. } => rows::letter_rows(ctx.layout),
        Page::Numbers => rows::number_rows(ctx.layout),
        _ => rows::symbol_rows(ctx.layout),
    };
    let mut first = row(m);
    for &c in keys[0] {
        first = first.child(cap_key(m, Cap::Char(c), Some(m.key_w), ctx, cx));
    }
    let mut second = row(m).justify_center();
    for &c in keys[1] {
        second = second.child(cap_key(m, Cap::Char(c), Some(m.key_w), ctx, cx));
    }
    let middle_w = if letters { m.key_w } else { wide_key_w(m) };
    let mut middle = div()
        .flex_1()
        .flex()
        .flex_row()
        .justify_center()
        .gap(m.px(m.key_gap));
    for &c in keys[2] {
        middle = middle.child(cap_key(m, Cap::Char(c), Some(middle_w), ctx, cx));
    }
    let toggle = match ctx.page {
        Page::Letters { .. } => Cap::Shift,
        Page::Numbers => Cap::Pages("#+=", Page::Symbols),
        _ => Cap::Pages("123", Page::Numbers),
    };
    let third = row(m)
        .child(cap_key(m, toggle, Some(side), ctx, cx))
        .child(middle)
        .child(cap_key(m, Cap::Delete, Some(side), ctx, cx));
    let pages = match letters {
        true => Cap::Pages("123", Page::Numbers),
        false => Cap::Pages("ABC", Page::Letters { shift: false }),
    };
    let mut bottom = row(m)
        .child(cap_key(m, pages, Some(side), ctx, cx))
        .child(cap_key(m, Cap::Emoji, Some(side), ctx, cx))
        .child(cap_key(m, Cap::Space, None, ctx, cx))
        .child(cap_key(m, Cap::Return, Some(m.key_w * RETURN), ctx, cx));
    // In landscape the globe and dictation sit beside the bottom row, in
    // the margins clear of the screen's corners.
    if !m.tall {
        let size = (m.margin - 8.0).clamp(1.0, 24.0);
        let inset = (m.margin - size) / 2.0;
        bottom = bottom
            .child(
                div()
                    .absolute()
                    .left(m.px(inset))
                    .top(m.px((m.key_h - size) / 2.0))
                    .child(layout_switch(m, IconName::Globe, size, ctx.layout, cx)),
            )
            .child(
                div()
                    .absolute()
                    .right(m.px(inset))
                    .top(m.px((m.key_h - size) / 2.0))
                    .child(icon(m, IconName::Mic, size, m.palette.label)),
            );
    }
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(m.px(m.top_pad))
        .gap(m.px(m.row_gap))
        .child(first)
        .child(second)
        .child(third)
        .child(bottom)
}

/// Below the keys: the globe, which switches layout, and dictation under an
/// iPhone's portrait keys; the home indicator under every iOS keyboard.
pub(super) fn strip(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let indicator = div()
        .absolute()
        .bottom(m.px(8.0))
        .left(m.px((m.width - 134.0) / 2.0))
        .w(m.px(134.0))
        .h(m.px(5.0))
        .rounded(m.px(2.5))
        .bg(hex(m.palette.label));
    let icons = m.style == Style::Iphone && m.tall;
    let top = (m.strip - 13.0 - STRIP_ICON) / 2.0;
    let edge = m.margin + m.key_w * 0.35;
    div()
        .relative()
        .flex_1()
        .min_h(m.px(m.strip))
        .when(icons, |this| {
            this.child(
                div()
                    .absolute()
                    .top(m.px(top))
                    .left(m.px(edge))
                    .child(layout_switch(
                        m,
                        IconName::Globe,
                        STRIP_ICON,
                        ctx.layout,
                        cx,
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top(m.px(top))
                    .right(m.px(edge))
                    .child(icon(m, IconName::Mic, STRIP_ICON, m.palette.label)),
            )
        })
        .child(indicator)
}
