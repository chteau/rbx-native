//! The popover's small pieces: gradients, thumbs, check rows, notes and
//! buttons.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::accent::Check;
use crate::tokens;

use super::super::super::kit::{icon, mono, text};

pub(super) fn gradient(angle: f32, from: impl Into<Hsla>, to: impl Into<Hsla>) -> Background {
    linear_gradient(
        angle,
        linear_color_stop(from, 0.),
        linear_color_stop(to, 1.),
    )
}

/// A picker thumb: 14 px, a 2 px white ring, a hairline dark halo.
pub(super) fn thumb(fill: Rgba) -> Div {
    div()
        .absolute()
        .size(px(14.))
        .rounded_full()
        .border_2()
        .border_color(rgb(0xFFFFFF))
        .bg(fill)
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.4),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        }])
}

/// Records where an element was laid out, for mapping a pointer onto it.
pub(super) fn measure(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
        .absolute()
        .size_full()
}

pub(super) fn check_row(check: &Check) -> Div {
    let (glyph, color) = if check.passes() {
        ("check", tokens::diff_add())
    } else {
        ("x", tokens::text_error())
    };
    h_flex()
        .h(px(20.))
        .gap(px(8.))
        .items_center()
        .child(div().text_color(color).child(icon(glyph, 11.)))
        .child(
            div()
                .flex_1()
                .text_size(px(11.5))
                .text_color(tokens::text2())
                .child(check.label),
        )
        .child(
            // "4.5", "3": the bar as it is usually written.
            mono(10.5, 14.)
                .text_color(color)
                .child(format!("{:.1} / {}", check.ratio, check.min)),
        )
}

pub(super) fn note(color: Rgba, glyph: &'static str, body: impl IntoElement) -> Div {
    h_flex()
        .gap(px(8.))
        .p(px(10.))
        .items_start()
        .rounded(px(6.))
        .bg(Rgba { a: 0.08, ..color })
        .border_1()
        .border_color(Rgba { a: 0.22, ..color })
        .child(div().text_color(color).child(icon(glyph, 14.)))
        .child(
            text(11.5, 16.)
                .flex_1()
                .min_w_0()
                .text_color(tokens::text2())
                .child(body),
        )
}

pub(super) fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    fill: Option<Rgba>,
) -> Stateful<Div> {
    let base = h_flex()
        .id(id)
        .flex_none()
        .h(px(28.))
        .items_center()
        .rounded(px(5.))
        .text_size(px(12.))
        .line_height(px(16.))
        .cursor_pointer()
        .child(label.into());
    match fill {
        Some(fill) => base
            .px(px(12.))
            .bg(fill)
            .text_color(tokens::black())
            .font_weight(FontWeight::BOLD),
        None => base
            .px(px(10.))
            .border_1()
            .border_color(tokens::border2())
            .bg(tokens::field_select())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|this| this.bg(rgba(0x202123FF))),
    }
}
