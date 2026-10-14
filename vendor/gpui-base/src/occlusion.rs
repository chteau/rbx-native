//! rbx-native addition: where popups were painted this frame, per window.
//!
//! A native child window embedded in a GPUI window (the Roblox client during
//! Play) is drawn by the X server over everything GPUI paints, popups
//! included. Every floating layer (popups, tooltips, dialog hosts, and the
//! application's own `deferred` popups) carries a `probe()`, which records
//! its painted bounds; the application reads them with `rects` and cuts
//! those areas out of the embedded window.
//!
//! The application calls `begin_frame` for a window at the start of its root
//! view's render, so the list only ever holds what the current frame drew.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{canvas, Bounds, IntoElement, ParentElement as _, Pixels, Styled as _, WindowId};

thread_local! {
    static RECTS: RefCell<HashMap<u64, Vec<Bounds<Pixels>>>> = RefCell::new(HashMap::new());
}

/// An invisible element filling its parent that records the parent's
/// bounds (window coordinates, logical pixels) for its window on prepaint.
/// Put it as the last child of a `relative` container.
pub fn probe() -> impl IntoElement {
    canvas(
        |bounds, window, _| {
            let id = window.window_handle().window_id().as_u64();
            RECTS.with(|rects| rects.borrow_mut().entry(id).or_default().push(bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Forgets what earlier frames recorded for `window`.
pub fn begin_frame(window: WindowId) {
    RECTS.with(|rects| {
        rects.borrow_mut().remove(&window.as_u64());
    });
}

/// What this frame has recorded so far for `window`.
pub fn rects(window: WindowId) -> Vec<Bounds<Pixels>> {
    RECTS.with(|rects| rects.borrow().get(&window.as_u64()).cloned().unwrap_or_default())
}

/// `probe` wrapped around `child`: a `relative` div sized by the child.
pub fn wrap(child: impl IntoElement) -> impl IntoElement {
    gpui::div().relative().child(child).child(probe())
}

