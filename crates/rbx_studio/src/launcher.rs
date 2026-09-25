//! The launcher: the three windows a bare `rbxstudio` opens before any
//! place is loaded — the API key setup wizard (first launch), Home (every
//! launch after), and Roblox publishing (the key, from Home's sidebar card).
//! What they stand on is
//! `crate::home` and `crate::key_store`.
//!
//! Only one launcher window is open at a time and it is the app's only
//! window, so closing it quits; handing over to the editor closes it
//! without quitting.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::*;

use crate::tokens;

mod fixtures;
mod home_window;
mod key_check;
mod publishing;
mod ui;
mod wizard;

pub(crate) use home_window::HomeWindow;

/// What the editor window needs beyond the place, held from `main` until a
/// place is picked. Taken exactly once.
pub(crate) type Boot = Rc<RefCell<Option<crate::EditorBoot>>>;

fn options(
    title: &'static str,
    size: (f32, f32),
    min: Option<(f32, f32)>,
    cx: &App,
) -> WindowOptions {
    let window_size = gpui_kit::size(tokens::scaled_width(size.0), tokens::scaled_width(size.1));
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        is_resizable: min.is_some(),
        window_min_size: min.map(|(w, h)| gpui_kit::size(px(w), px(h))),
        app_owns_titlebar_drag: true,
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}

/// First launch: the setup wizard (1040×720, fixed).
pub(crate) fn open_wizard(boot: Boot, cx: &mut App) {
    let options = options("Set up publishing", (1040., 720.), None, cx);
    let _ = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| wizard::Wizard::new(boot, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
}

/// Home (1440×900, resizable down to 1100×700).
pub(crate) fn open_home(boot: Boot, cx: &mut App) {
    let options = options("RbxNative", (1440., 900.), Some((1100., 700.)), cx);
    let _ = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| HomeWindow::new(boot, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
}

/// Roblox publishing (760×720, height resizable), over Home. `on_change`
/// runs after the key was replaced or removed, so Home can reload.
pub(crate) fn open_publishing(on_change: impl Fn(&mut App) + 'static, cx: &mut App) {
    let options = options("Roblox publishing", (760., 720.), Some((760., 520.)), cx);
    let on_change = Rc::new(on_change);
    let _ = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| publishing::Publishing::new(on_change, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
}
