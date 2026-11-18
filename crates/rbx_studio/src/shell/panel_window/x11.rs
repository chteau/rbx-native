//! What X11 leaves a torn-out dock's window to do for itself.
//!
//! GPUI's X11 back end only makes a floating window transient for the
//! window that has the keyboard as it opens, and asks the window manager
//! for no position at all. So a dock that opens before the editor has the
//! keyboard (a widget that floats from its first run, a layout restored at
//! launch) can sink behind it, and a plugin's window opens wherever the
//! window manager likes rather than where it was left. Both are set once
//! the window is mapped, which GPUI does some time after opening it.

use std::time::Duration;

use gpui_kit::component::Root;
use gpui_kit::*;
use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// Makes `window` transient for the editor, and moves it to `origin` when
/// it has one.
pub(super) fn settle(
    window: WindowHandle<Root>,
    editor: Option<AnyWindowHandle>,
    origin: Option<Point<Pixels>>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        for wait in [100, 400] {
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            let Some(main) = editor else {
                return;
            };
            let editor = main.update(cx, |_, window, _| crate::pointer_lock::window_id(window));
            let Ok(Some(editor)) = editor else {
                return;
            };
            let _ = window.update(cx, |_, window, _| {
                let Some(id) = crate::pointer_lock::window_id(window) else {
                    return;
                };
                let Ok((connection, _)) = x11rb::connect(None) else {
                    return;
                };
                // Each one waited on: a request left in the buffer as the
                // connection drops never reaches the server.
                let _ = connection
                    .change_property32(
                        PropMode::REPLACE,
                        id,
                        AtomEnum::WM_TRANSIENT_FOR,
                        AtomEnum::WINDOW,
                        &[editor],
                    )
                    .map(|cookie| cookie.check());
                if let Some(origin) = origin {
                    let scale = window.scale_factor();
                    let aux = ConfigureWindowAux::new()
                        .x((f32::from(origin.x) * scale) as i32)
                        .y((f32::from(origin.y) * scale) as i32);
                    let _ = connection
                        .configure_window(id, &aux)
                        .map(|cookie| cookie.check());
                }
            });
        }
    })
    .detach();
}
