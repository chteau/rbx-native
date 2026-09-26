//! rbx-native addition: a per-line slot in the line-number gutter, for
//! breakpoint markers, and one highlighted line, for where a paused script
//! stopped. Upstream's gutter has line numbers and fold chevrons only.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Hsla, InteractiveElement as _, IntoElement as _, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Styled as _, Window, point, size,
};

use super::layout::LastLayout;

/// Draws the marker for a 0-based buffer line, or nothing.
pub type GutterMarker = Rc<dyn Fn(usize, &mut Window, &mut App) -> Option<AnyElement>>;
/// A mouse button pressed over a 0-based buffer line's gutter cell.
pub type GutterClick = Rc<dyn Fn(usize, MouseButton, &MouseDownEvent, &mut Window, &mut App)>;

/// What the gutter shows beside the line numbers.
#[derive(Clone)]
pub struct Gutter {
    pub marker: GutterMarker,
    pub on_click: GutterClick,
    /// A 0-based buffer line painted edge to edge in this colour.
    pub highlight: Option<(usize, Hsla)>,
}

/// One clickable cell per visible line, over the line-number column,
/// prepainted and ready to paint.
pub(super) fn layout(
    gutter: &Gutter,
    origin_x: Pixels,
    bounds: &Bounds<Pixels>,
    last_layout: &LastLayout,
    width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let line_height = last_layout.line_height;
    let mut offset_y = last_layout.visible_top;
    let mut cells = Vec::with_capacity(last_layout.visible_buffer_lines.len());
    for (line, &buffer_line) in last_layout
        .lines
        .iter()
        .zip(last_layout.visible_buffer_lines.iter())
    {
        let on_click = gutter.on_click.clone();
        let mut cell = gpui::div()
            .id(("rbx-gutter", buffer_line))
            .w(width)
            .h(line_height)
            .flex()
            .items_center()
            .children((gutter.marker)(buffer_line, window, cx))
            .on_any_mouse_down(move |event, window, cx| {
                cx.stop_propagation();
                on_click(buffer_line, event.button, event, window, cx);
            })
            .into_any_element();
        let origin = point(origin_x, bounds.origin.y + offset_y);
        cell.prepaint_as_root(origin, size(width, line_height).into(), window, cx);
        cells.push(cell);
        offset_y += line.wrapped_lines.len() * line_height;
    }
    cells
}
