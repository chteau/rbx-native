//! Small pieces more than one part of the window draws.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon};
use crate::tokens;

/// One segment of a segmented control: the accent wash and text colour
/// when selected.
pub(super) fn segment(
    id: impl Into<ElementId>,
    label: &'static str,
    selected: bool,
    h: f32,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .h(px(h))
        .px(px(11.))
        .items_center()
        .rounded(px(4.))
        .text_size(px(11.5))
        .line_height(px(16.))
        .map(|this| {
            if selected {
                this.bg(tokens::accent_soft())
                    .text_color(tokens::text())
                    .font_weight(FontWeight::SEMIBOLD)
            } else {
                this.text_color(tokens::text2())
                    .cursor_pointer()
                    .hover(|this| this.bg(ui::wash()))
            }
        })
        .child(label)
}

/// A 24 px ghost button holding one glyph, in text3.
pub(super) fn ghost_glyph(
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .size(px(24.))
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(tokens::text3())
        .cursor_pointer()
        .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
        .child(icon(glyph, 13.))
        .tooltip(move |window, cx| super::super::tooltip::text(label, window, cx))
}

/// The class, as a neutral tag: a starter's class can't change.
pub(super) fn class_tag(class: &'static str) -> Div {
    tag_frame()
        .bg(ui::wash())
        .text_color(tokens::text2())
        .child(class)
}

fn tag_frame() -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px(px(6.))
        .items_center()
        .rounded(px(4.))
        .text_size(px(10.5))
        .line_height(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
}

/// `Built-in` (text2 on 5% white) or `Yours` (text on the accent wash).
pub(super) fn tag(yours: bool) -> Div {
    tag_frame().map(|this| {
        if yours {
            this.bg(tokens::accent_soft())
                .text_color(tokens::text())
                .child("Yours")
        } else {
            this.bg(ui::wash())
                .text_color(tokens::text2())
                .child("Built-in")
        }
    })
}

/// Opens the templates folder, creating it first: a fresh install has none
/// until the first template is written.
pub(super) fn open_folder(folder: Option<&std::path::Path>, cx: &mut App) {
    if let Some(folder) = folder {
        let _ = std::fs::create_dir_all(folder);
        cx.open_with_system(folder);
    }
}
