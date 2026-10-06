//! Headless layout measurement for the 44x44 floor (WCAG 2.5.5).
//!
//! Each primary or destructive control tags its own element with
//! `debug_selector` (a no-op outside tests), so this measures the element a
//! click actually lands on, laid out inside a real gpui window, rather than
//! trusting a number written next to it.

use gpui_kit::prelude::*;
use gpui_kit::*;

type Build = Box<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// A root view that renders whatever the test hands it, rebuilt every frame
/// (so entities such as a popup menu can be made inside the window).
struct Holder(Build);

impl Render for Holder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        (self.0)(window, cx)
    }
}

/// Lays out `build()` inside a container `container_height` tall and returns
/// the bounds of every selector in `selectors` that was found.
pub(crate) fn bounds_of<E: IntoElement>(
    cx: &mut TestAppContext,
    selectors: &[&'static str],
    container_height: f32,
    build: impl Fn(&mut Window, &mut App) -> E + 'static,
) -> Vec<(&'static str, Size<Pixels>)> {
    cx.update(gpui_kit::init);
    let (_view, cx) = cx.add_window_view(move |_, _| {
        Holder(Box::new(move |window, cx| {
            div()
                .flex()
                .items_start()
                .w(px(900.))
                .h(px(container_height))
                .child(build(window, cx))
                .into_any_element()
        }))
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    selectors
        .iter()
        .filter_map(|&selector| Some((selector, cx.debug_bounds(selector)?.size)))
        .collect()
}

/// The size of the one element tagged `selector`.
pub(crate) fn size_of<E: IntoElement>(
    cx: &mut TestAppContext,
    selector: &'static str,
    container_height: f32,
    build: impl Fn(&mut Window, &mut App) -> E + 'static,
) -> Size<Pixels> {
    bounds_of(cx, &[selector], container_height, build)
        .pop()
        .unwrap_or_else(|| panic!("nothing tagged {selector:?} was laid out"))
        .1
}

/// Asserts the floor. Scale and Large Click Targets are process-wide and
/// other tests move them, so this does not: `tokens::tests` covers
/// `primary_target()` at every scale, and a control that clears 44 here has
/// no reason to depend on either setting.
pub(crate) fn assert_primary<E: IntoElement>(
    cx: &mut TestAppContext,
    selector: &'static str,
    container_height: f32,
    build: impl Fn(&mut Window, &mut App) -> E + 'static,
) {
    let found = size_of(cx, selector, container_height, build);
    assert!(
        f32::from(found.width) >= 44. && f32::from(found.height) >= 44.,
        "{selector} is {}x{}, under WCAG 2.5.5's 44x44",
        f32::from(found.width),
        f32::from(found.height),
    );
}
