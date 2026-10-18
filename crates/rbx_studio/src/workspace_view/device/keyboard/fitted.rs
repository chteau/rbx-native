//! The iPad's and Gboard's key pages, whose rows span the whole width with
//! keys of set proportions (`fit`), and Android's navigation bar below the
//! keys.

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::super::WorkspaceView;
use super::{cap_key, hex, icon, layout_switch, rows, Cap, Ctx, Metrics, Page};

/// A row of `(cap, width in units)`, scaled so it spans the cover, with
/// `pad` units of empty room before and after the keys.
fn fit(
    m: &Metrics,
    keys: &[(Cap, f32)],
    (before, after): (f32, f32),
    ctx: &Ctx,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let units: f32 = keys.iter().map(|(_, w)| w).sum::<f32>() + before + after;
    let gaps = keys.len().saturating_sub(1) as f32;
    let unit = ((m.width - 2.0 * m.margin - gaps * m.key_gap) / units.max(1.0)).max(0.0);
    let mut row = div()
        .flex()
        .flex_row()
        .flex_none()
        .w_full()
        .h(m.px(m.key_h))
        .pl(m.px(m.margin + before * unit))
        .pr(m.px(m.margin + after * unit))
        .gap(m.px(m.key_gap));
    for &(cap, w) in keys {
        row = row.child(cap_key(m, cap, Some(w * unit), ctx, cx));
    }
    row
}

fn chars(labels: &[&'static str]) -> Vec<(Cap, f32)> {
    labels.iter().map(|&c| (Cap::Char(c), 1.0)).collect()
}

fn stack(m: &Metrics, rows: [Div; 4]) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(m.px(m.top_pad))
        .gap(m.px(m.row_gap))
        .children(rows)
}

/// The iPad's keyboard: delete and return at the ends of the top rows, a
/// shift at each end of the third, and the page, globe, dictation and
/// hide keys around the space bar.
pub(super) fn ipad(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let letters = matches!(ctx.page, Page::Letters { .. });
    let keys = match ctx.page {
        Page::Letters { .. } => rows::letter_rows(ctx.layout),
        Page::Numbers => rows::number_rows(ctx.layout),
        _ => rows::symbol_rows(ctx.layout),
    };
    let mut first = chars(keys[0]);
    first.push((Cap::Delete, 1.5));
    let mut second = chars(keys[1]);
    second.push((Cap::Return, if keys[1].len() < 10 { 2.0 } else { 1.5 }));
    // A nine-key home row starts half a key in, as on the iPad.
    let indent = if keys[1].len() < 10 { 0.5 } else { 0.0 };
    let toggle = match ctx.page {
        Page::Letters { .. } => Cap::Shift,
        Page::Numbers => Cap::Pages("#+=", Page::Symbols),
        _ => Cap::Pages("123", Page::Numbers),
    };
    let mut third = vec![(toggle, 1.3)];
    third.extend(chars(keys[2]));
    if letters {
        third.extend(chars(&[",", "."]));
    }
    third.push((toggle, 1.3));
    let pages = match letters {
        true => Cap::Pages(".?123", Page::Numbers),
        false => Cap::Pages("ABC", Page::Letters { shift: false }),
    };
    let bottom = [
        (pages, 1.6),
        (Cap::Globe, 1.0),
        (Cap::Mic, 1.0),
        (Cap::Space, 5.6),
        (pages, 1.6),
        (Cap::Dismiss, 1.0),
    ];
    stack(
        m,
        [
            fit(m, &first, (0.0, 0.0), ctx, cx),
            fit(m, &second, (indent, 0.0), ctx, cx),
            fit(m, &third, (0.0, 0.0), ctx, cx),
            fit(m, &bottom, (0.0, 0.0), ctx, cx),
        ],
    )
}

/// Gboard: the letters in iOS's places, shift and delete flanking the third
/// row, and ?123, comma, emoji, space, full stop and the round return pill
/// at the bottom.
pub(super) fn android(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let letters = matches!(ctx.page, Page::Letters { .. });
    let keys = match ctx.page {
        Page::Letters { .. } => rows::letter_rows(ctx.layout),
        Page::Numbers => rows::number_rows(ctx.layout),
        _ => rows::symbol_rows(ctx.layout),
    };
    // A nine-key row keeps ten-key widths and centres, half a key each side.
    let indent = if keys[1].len() < 10 { 0.5 } else { 0.0 };
    let toggle = match ctx.page {
        Page::Letters { .. } => Cap::Shift,
        Page::Numbers => Cap::Pages("=\\<", Page::Symbols),
        _ => Cap::Pages("?123", Page::Numbers),
    };
    let middle = if letters { 1.0 } else { 7.0 / 5.0 };
    let mut third = vec![(toggle, 1.5)];
    third.extend(keys[2].iter().map(|&c| (Cap::Char(c), middle)));
    third.push((Cap::Delete, 1.5));
    let pages = match letters {
        true => Cap::Pages("?123", Page::Numbers),
        false => Cap::Pages("ABC", Page::Letters { shift: false }),
    };
    let bottom = [
        (pages, 1.5),
        (Cap::Char(","), 1.0),
        (Cap::Emoji, 1.0),
        (Cap::Space, 4.0),
        (Cap::Char("."), 1.0),
        (Cap::Return, 1.5),
    ];
    stack(
        m,
        [
            fit(m, &chars(keys[0]), (0.0, 0.0), ctx, cx),
            fit(m, &chars(keys[1]), (indent, indent), ctx, cx),
            fit(m, &third, (0.0, 0.0), ctx, cx),
            fit(m, &bottom, (0.0, 0.0), ctx, cx),
        ],
    )
}

/// Android's gesture navigation bar under the keyboard: Gboard's hide
/// chevron at the left, its keyboard switcher (here: QWERTY ⇄ AZERTY) at
/// the right, and the gesture handle between.
pub(super) fn nav_bar(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let size = (m.strip * 0.5).clamp(1.0, 20.0);
    div()
        .flex_1()
        .min_h(m.px(m.strip))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(m.px(m.margin + 14.0))
        .child(icon(m, IconName::ChevronDown, size, m.palette.label))
        .child(
            div()
                .w(m.px(108.0))
                .h(m.px(4.0))
                .rounded(m.px(2.0))
                .bg(hex(m.palette.faint)),
        )
        .child(layout_switch(m, IconName::Keyboard, size, ctx.layout, cx))
}
