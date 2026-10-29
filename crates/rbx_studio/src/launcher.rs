//! The launcher: the three windows a bare `rbxstudio` opens before any
//! place is loaded, the API key setup wizard (first launch), Home (every
//! launch after), and Roblox publishing (the key, from Home's sidebar card).
//! What they stand on is
//! `crate::home` and `crate::key_store`.
//!
//! Only one launcher window is open at a time and it is the app's only
//! window, so closing it quits; handing over to the editor closes it
//! without quitting. The one exception is Home's My Games reopened over the
//! editor as its game picker ([`open_game_picker`]), which only closes.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::*;

use crate::tokens;

mod cache;
mod fixtures;
mod home_window;
mod key_check;
mod publishing;
pub(crate) mod setup;
pub(crate) mod startup;
pub(crate) mod ui;
mod wizard;

pub(crate) use cache::{cached_games, read_icon as cached_icon};
pub(crate) use home_window::HomeWindow;
pub(crate) use key_check::{
    summary as key_summary, KeyCheck, Summary as KeySummary, Tag as KeyTag,
};

/// A check of the stored key, started now, Settings › Account's card.
pub(crate) fn check_stored_key(cx: &mut App) -> Entity<KeyCheck> {
    cx.new(|cx| KeyCheck::with_key(key_check::stored_secret(), cx))
}

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
        window_background: crate::theme::active().effects.window,
        ..Default::default()
    }
}

/// First launch: the setup wizard (1040×720, fixed).
/// The groups the last signed-in account belongs to, as Home last listed
/// them; empty until Home has loaded once. Used to offer a group as the
/// creator of an upload without a network call.
pub(crate) fn cached_groups() -> Vec<rbx_cloud::Group> {
    cache::GamesCache::last_user()
        .and_then(cache::GamesCache::load)
        .map(|c| c.listing.groups)
        .unwrap_or_default()
}

/// Runs when a setup opened over the editor finishes or is closed, in place
/// of opening Home.
pub(crate) type Finish = Rc<dyn Fn(&mut App)>;

/// The setup wizard showing `plan`'s steps. With `on_finish`, it was opened
/// over the editor: finishing or closing it runs that and only closes.
pub(crate) fn open_wizard(
    boot: Boot,
    plan: setup::SetupPlan,
    on_finish: Option<Finish>,
    cx: &mut App,
) -> Option<WindowHandle<Root>> {
    let title = if plan.key {
        "Set up publishing"
    } else {
        "Sign in to Roblox"
    };
    let options = options(title, (1040., 720.), None, cx);
    cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| wizard::Wizard::new(boot, plan, on_finish, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .ok()
}

/// Home (1440×900, resizable down to 1100×700).
pub(crate) fn open_home(boot: Boot, cx: &mut App) {
    let options = options("RbxNative", (1440., 900.), Some((1100., 700.)), cx);
    let _ = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| HomeWindow::new(boot, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
}

/// The editor's game picker (1040×720, resizable down to 760×520): Home's
/// My Games, where picking a game, or adding one by place ID or link, hands
/// it to `on_pick` and closes the picker.
pub(crate) fn open_game_picker(
    on_pick: impl Fn(rbx_cloud::Experience, &mut App) + 'static,
    cx: &mut App,
) -> Option<WindowHandle<Root>> {
    let options = options(
        "Link to a Roblox place",
        (1040., 720.),
        Some((760., 520.)),
        cx,
    );
    cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| HomeWindow::picker(Rc::new(on_pick), window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .ok()
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
