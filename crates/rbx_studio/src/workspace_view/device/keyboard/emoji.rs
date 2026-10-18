//! The emoji page, its emoji drawn from Twemoji.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::super::WorkspaceView;
use super::{hex, icon, turns_to, types, Ctx, KeyTap, Metrics, Page};
use crate::device_sim::KeyboardLayout;

/// The "frequently used" page iOS opens on, as Twemoji file names (the
/// code point in hex): drawn as images, as the system's emoji font is only
/// reached through GPUI's fallback, which takes DejaVu Sans's monochrome
/// faces first on Linux.
pub(super) const EMOJI: &[&str] = &[
    "1f602", "2764", "1f60d", "1f923", "1f60a", "1f64f", "1f495", "1f62d", "1f618", "1f44d",
    "1f605", "1f44f", "1f601", "1f525", "1f970", "1f494", "1f496", "1f499", "1f622", "1f914",
    "1f606", "1f644", "1f4aa", "1f609", "263a", "1f44c", "1f917", "1f49c", "1f614", "1f60e",
    "1f607", "1f339", "1f926", "1f389", "1f49e", "270c", "2728", "1f937", "1f631", "1f60c",
    "1f338", "1f64c", "1f60b", "1f497", "1f49a", "1f60f", "1f49b", "1f642", "1f493", "1f929",
    "1f604", "1f600", "1f5a4", "1f603", "1f4af", "1f648", "1f447", "1f3b6", "1f612", "1f92d",
];

/// The emoji page's category bar, after ABC.
pub(super) const CATEGORIES: &[&str] = &[
    "1f552", "1f600", "1f43b", "1f354", "26bd", "1f697", "1f4a1", "1f523", "1f3f3",
];

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/twemoji"]
#[include = "*.svg"]
struct Twemoji;

/// Rasterized once per name, at a size that stays sharp on the largest
/// cover (an iPad's, on a 2x display).
pub(super) fn twemoji(name: &str) -> Option<Arc<RenderImage>> {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Option<Arc<RenderImage>>>> =
            RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|cache| {
        cache
            .entry(name.to_owned())
            .or_insert_with(|| {
                let file = Twemoji::get(&format!("{name}.svg"))?;
                crate::class_icons::rasterize_at(&file.data, 96)
            })
            .clone()
    })
}

fn emoji(name: &str, size: Pixels) -> Div {
    div()
        .size(size)
        .flex_none()
        .when_some(twemoji(name), |this, image| {
            this.child(img(image).size_full())
        })
}

/// The character a Twemoji file name stands for.
pub(super) fn character(name: &str) -> Option<char> {
    u32::from_str_radix(name, 16).ok().and_then(char::from_u32)
}

/// The "frequently used" grid, a header above it and the category bar
/// below, in the room the suggestion bar and the key rows take.
pub(super) fn page(m: &Metrics, ctx: &Ctx, cx: &mut Context<WorkspaceView>) -> Div {
    let p = m.palette;
    let header = match ctx.layout {
        KeyboardLayout::Qwerty => "FREQUENTLY USED",
        KeyboardLayout::Azerty => "UTILIS\u{c9}S FR\u{c9}QUEMMENT",
    };
    let height = m.suggest + m.rows_height();
    let bar = m.key_h.min(36.0);
    let title = 24.0;
    let cell = 42f32.min((height - bar - title) / 5.0).max(1.0);
    let rows = ((height - bar - title) / cell).floor().max(1.0) as usize;
    let columns = ((m.width - 2.0 * m.margin) / cell).floor().max(1.0) as usize;
    // Filled column by column, as iOS and Gboard scroll the grid sideways.
    let mut grid = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .px(m.px(m.margin));
    for r in 0..rows {
        let mut line = div().flex().flex_row().h(m.px(cell));
        for c in 0..columns {
            let mut slot = div()
                .w(m.px(cell))
                .h_full()
                .flex()
                .items_center()
                .justify_center();
            if let Some(name) = EMOJI.get(c * rows + r).copied() {
                slot = slot.child(emoji(name, m.px(cell * 0.78)));
                if let Some(ch) = character(name) {
                    slot = types(slot.cursor_pointer(), KeyTap::Text(ch.to_string()), cx);
                }
            }
            line = line.child(slot);
        }
        grid = grid.child(line);
    }
    let categories = div()
        .h(m.px(bar))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .px(m.px(m.margin + 6.0))
        .gap(m.px(4.0))
        .child(turns_to(
            div()
                .flex_none()
                .pr(m.px(6.0))
                .cursor_pointer()
                .text_size(m.px(15.0))
                .text_color(hex(p.label))
                .child("ABC"),
            Page::Letters { shift: false },
            cx,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .justify_between()
                .opacity(0.55)
                .children(CATEGORIES.iter().map(|&name| emoji(name, m.px(bar * 0.5)))),
        )
        .child(types(
            div().pl(m.px(6.0)).cursor_pointer().child(icon(
                m,
                IconName::Delete,
                bar * 0.6,
                p.label,
            )),
            KeyTap::Backspace,
            cx,
        ));
    div()
        .h(m.px(height))
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .h(m.px(title))
                .flex_none()
                .flex()
                .items_end()
                .px(m.px(m.margin + 6.0))
                .text_size(m.px(12.0))
                .text_color(hex(p.faint))
                .child(header),
        )
        .child(grid)
        .child(categories)
}
