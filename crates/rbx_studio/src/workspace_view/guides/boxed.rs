//! Studio's floating measurement box (`FloatingValueInput`), as the read-only
//! label a handle drag shows and the editable box a Move drag leaves
//! behind (see `workspace_view::measure`) both draw it.

use gpui_kit::*;

/// The measurement text's size: Studio's `SourceSansBold` 24.
pub(in crate::workspace_view) fn label_text() -> Pixels {
    px(24.0 * crate::tokens::font_scale())
}

/// Studio's floating measurement box, legacy dark theme: a white bold number
/// on RGB(37, 37, 37) with a `border` and small rounded corners
/// (`FloatingValueInput`), padded 4/4/2/4.
pub(in crate::workspace_view) fn measurement_box(border: Rgba) -> Div {
    div()
        .pl(px(4.0))
        .pr(px(4.0))
        .pt(px(2.0))
        .pb(px(4.0))
        .bg(rgb(0x252525))
        .border_1()
        .border_color(border)
        .rounded(px(3.0))
        .text_color(rgb(0xffffff))
        .text_size(label_text())
        .line_height(label_text())
        .font_weight(FontWeight::BOLD)
}

/// `element` centred on `at`: a slot wider and taller than any box, centred
/// on the anchor, with the box centred inside it — GPUI places a child by
/// its corner, not its middle.
pub(in crate::workspace_view) fn centred_on(
    at: Point<Pixels>,
    element: impl IntoElement,
) -> impl IntoElement {
    const SLOT: f32 = 200.0;
    div()
        .absolute()
        .left(at.x - px(SLOT / 2.0))
        .top(at.y - px(SLOT / 2.0))
        .size(px(SLOT))
        .flex()
        .items_center()
        .justify_center()
        .child(element)
}

/// The distance label a drag shows, read-only.
pub(in crate::workspace_view) fn label_element(
    at: Point<Pixels>,
    text: SharedString,
) -> impl IntoElement {
    centred_on(at, measurement_box(rgb(0x000000)).child(text))
}
