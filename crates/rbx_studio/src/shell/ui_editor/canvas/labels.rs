//! The numbers the canvas pins to what it draws: a distance over its run,
//! and the selection's size under its frame.

use gpui_kit::*;

use crate::tokens;

/// A distance's number, over the middle of its run.
pub(super) fn distance_label(at: [f32; 2], length: f32) -> AnyElement {
    pill(tokens::tool_scale(), format!("{}", length.round()))
        .absolute()
        .left(px(at[0] + 4.0))
        .top(px(at[1] + 4.0))
        .into_any_element()
}

/// The selection's width by height in canvas pixels, centred under the
/// point `under` — Figma's size pill.
pub(super) fn size_badge(under: [f32; 2], size: [f32; 2]) -> AnyElement {
    // Wide enough for any size a screen can have, and centred on `under`
    // by laying the pill out in the middle of it.
    const SPAN: f32 = 160.0;
    let [w, h] = size.map(|length| length.round());
    div()
        .absolute()
        .left(px(under[0] - SPAN * 0.5))
        .top(px(under[1] + 8.0))
        .w(px(SPAN))
        .flex()
        .justify_center()
        .child(pill(tokens::check_on(), format!("{w} × {h}")))
        .into_any_element()
}

fn pill(background: Rgba, text: String) -> Div {
    div()
        .px(px(3.))
        .rounded(tokens::RADIUS_TINY)
        .bg(background)
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .text_color(tokens::black())
        .child(text)
}

/// A small tag pinned at `at`: an auto layout child's place in it, or what
/// the layout is.
pub(super) fn tag(at: [f32; 2], text: String) -> AnyElement {
    pill(tokens::tool_scale(), text)
        .absolute()
        .left(px(at[0]))
        .top(px(at[1]))
        .into_any_element()
}
