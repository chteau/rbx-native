//! The sheet's small parts — its text and icon buttons, a separator, mono
//! text — and the arithmetic of its readouts and its stage.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

/// What the stage keeps clear of the area's edges, each side.
const STAGE_MARGIN: f32 = 23.0;

/// A text button's three looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::ui_editor) enum Look {
    Ghost,
    On,
    Primary,
}

/// An icon and a label, 24 tall — the boards' one text button.
pub(in crate::shell::ui_editor) fn text_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    look: Look,
) -> Stateful<Div> {
    h_flex()
        .id(id.into())
        .flex_none()
        .h(px(24.))
        .items_center()
        .gap(px(5.))
        .pl(px(6.))
        .pr(px(8.))
        .rounded(px(4.))
        .cursor_pointer()
        .text_size(tokens::text_md())
        .line_height(px(16.))
        .map(|this| match look {
            Look::Ghost => this
                .font_weight(FontWeight::MEDIUM)
                .text_color(tokens::text2())
                .hover(|this| this.bg(tokens::hover()).text_color(tokens::text())),
            Look::On => this
                .font_weight(FontWeight::MEDIUM)
                .bg(tokens::accent_soft())
                .text_color(tokens::check_on()),
            // Near-black on the accent: white on it is under 4:1.
            Look::Primary => this
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .bg(tokens::check_on())
                .text_color(tokens::black())
                .hover(|this| this.bg(tokens::accent_hover())),
        })
        .child(Icon::new(icon).size(px(14.)))
        .child(label.into())
}

/// A 24px square icon button, lit while `on`.
pub(super) fn icon_tool(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
        .map(|this| match on {
            true => this
                .bg(tokens::accent_soft())
                .text_color(tokens::check_on()),
            false => this
                .text_color(tokens::text2())
                .hover(|this| this.bg(tokens::hover()).text_color(tokens::text())),
        })
        .tooltip(move |window, cx| crate::shell::tooltip::text(label, window, cx))
        .child(Icon::new(icon).size(px(14.)))
}

pub(in crate::shell::ui_editor) fn separator() -> Div {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(14.))
        .mx(px(4.))
        .bg(tokens::border2())
}

/// Mono text, `size` px, in `color`.
pub(super) fn mono(text: impl Into<SharedString>, size: f32, color: Rgba) -> Div {
    div()
        .flex_none()
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(px(size))
        .line_height(px(16.))
        .text_color(color)
        .child(text.into())
}

/// `width × height · w:h`, the aspect reduced.
pub(super) fn sized(size: [f32; 2]) -> String {
    let [width, height] = size.map(|side| side.round().max(1.0) as u32);
    let divisor = gcd(width, height);
    format!(
        "{width} × {height} · {}:{}",
        width / divisor,
        height / divisor
    )
}

fn gcd(a: u32, b: u32) -> u32 {
    match b {
        0 => a.max(1),
        b => gcd(b, a % b),
    }
}

/// The largest `aspect` box inside `area` less its margins, whole pixels.
pub(super) fn stage_size(area: Size<Pixels>, aspect: f32) -> (u32, u32) {
    let width = (f32::from(area.width) - 2.0 * STAGE_MARGIN).max(1.0);
    let height = (f32::from(area.height) - 2.0 * STAGE_MARGIN).max(1.0);
    let (width, height) = match width / height > aspect {
        true => (height * aspect, height),
        false => (width, width / aspect),
    };
    (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}
