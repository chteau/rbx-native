//! Headless layout measurement for the 44x44 floor (WCAG 2.5.5).
//!
//! Each primary or destructive control tags its own element with
//! `debug_selector` (a no-op outside tests), so this measures the element a
//! click actually lands on, drawn through real gpui layout, rather than
//! trusting a number written next to it.

use gpui_kit::prelude::*;
use gpui_kit::*;

/// Draws `element` inside a container `container_height` tall and returns the
/// size of the element tagged `selector`.
pub(crate) fn size_of(
    cx: &mut VisualTestContext,
    selector: &'static str,
    container_height: f32,
    element: impl IntoElement + 'static,
) -> Size<Pixels> {
    cx.draw(
        point(px(0.), px(0.)),
        size(
            AvailableSpace::Definite(px(900.)),
            AvailableSpace::Definite(px(600.)),
        ),
        move |_, _| {
            div()
                .flex()
                .items_start()
                .w(px(900.))
                .h(px(container_height))
                .child(element)
        },
    );
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing tagged {selector:?} was laid out"))
        .size
}

/// Asserts the floor at whatever scale the process is at. The scale and
/// Large Click Targets are process-wide and other tests move them, so this
/// does not: `tokens::tests` covers `primary_target()` at every scale, and a
/// control that clears 44 here has no reason to depend on either setting.
pub(crate) fn assert_primary(
    cx: &mut VisualTestContext,
    selector: &'static str,
    container_height: f32,
    element: impl IntoElement + 'static,
) {
    let found = size_of(cx, selector, container_height, element);
    assert!(
        f32::from(found.width) >= 44. && f32::from(found.height) >= 44.,
        "{selector} is {}x{}, under WCAG 2.5.5's 44x44",
        f32::from(found.width),
        f32::from(found.height),
    );
}
