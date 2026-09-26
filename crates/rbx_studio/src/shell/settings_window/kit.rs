//! What every Settings page is built from: a section title over a card of
//! rows, each row a label and description at the left and its control at
//! the right, and the handful of controls those rows carry. Sizes are the
//! design's, in pixels, like the other windows drawn after it (the launcher,
//! the Argon Diff window); only the window itself follows the UI scale.

use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::*;

use crate::tokens;

mod controls;
mod row;

pub(super) use controls::{
    ghost_icon, readout, secondary_button, segmented, slider, still_slider, still_toggle,
    ticked_slider, toggle, OnPick,
};
pub(super) use row::{section, section_card, Reset, Row, Section};

pub(super) fn text(size: f32, line: f32) -> Div {
    div().text_size(px(size)).line_height(px(line))
}

pub(super) fn mono(size: f32, line: f32) -> Div {
    text(size, line).font_family(tokens::FONT_FAMILY_MONO)
}

/// A Lucide glyph from the kit's catalogue, by file name.
pub(super) fn icon(name: &'static str, size: f32) -> Icon {
    Icon::empty()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
}

/// The `SOON` pill, with its "On the roadmap" tooltip.
pub(super) fn soon_pill(id: impl Into<ElementId>) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .h(px(16.))
        .px(px(5.))
        .items_center()
        .border_1()
        .border_color(tokens::border2())
        .rounded(px(4.))
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(9.5))
        .line_height(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens::text3())
        .child("SOON")
        .tooltip(|window, cx| super::super::tooltip::text("On the roadmap", window, cx))
}

/// A key cap: border2 with a 2 px bottom edge, mono 10.5.
pub(super) fn key_hint(keys: &'static str) -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px(px(5.))
        .items_center()
        .border_1()
        .border_b_2()
        .border_color(tokens::border2())
        .rounded(px(4.))
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(10.5))
        .line_height(px(12.))
        .text_color(tokens::text2())
        .child(keys)
}

pub(super) fn card() -> Div {
    v_flex()
        .border_1()
        .border_color(tokens::border())
        .rounded(px(8.))
        .bg(tokens::field_select())
        .overflow_hidden()
}

/// A page header's ghost button ("Reset page", "Restore defaults"): text2
/// with a reset glyph, or text3 and inert while there is nothing to do.
pub(super) fn header_button(
    id: &'static str,
    label: &'static str,
    on_click: Option<impl Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
) -> Stateful<Div> {
    let base = h_flex()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(px(5.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icon("rotate-ccw", 12.))
        .child(label);
    match on_click {
        Some(on_click) => base
            .text_color(tokens::text2())
            .cursor_pointer()
            .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
            .on_click(on_click),
        None => base.text_color(tokens::text3()),
    }
}
