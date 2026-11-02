//! The game workspace: every place of a game open together in one window,
//! which Roblox Studio (one place per window) has no equivalent of. See
//! `workspace` for the model; `bar` draws the place switcher, `find_window`
//! searches and replaces across every place's scripts, `sync_window` keeps
//! shared assets alike in every place, `summary` saves and publishes them
//! all.
//!
//! A game is a `.rbxgame` manifest beside its place files (`manifest`):
//! nothing is ever written into a place file but what the user edits there,
//! plus the shared-asset id attribute (`shared`), which is an ordinary
//! instance attribute the engine ignores.

use gpui_kit::component::Root;
use gpui_kit::*;

use crate::tokens;

mod bar;
mod find_window;
pub(crate) mod manifest;
mod open;
mod search;
mod shared;
mod summary;
mod sync_window;
mod workspace;

pub(crate) use find_window::QUERY_VARIABLE as FIND_VARIABLE;
pub(crate) use workspace::{open_game, GameWorkspace};

/// A floating window of the game's (Find in Game, Sync Shared) holding `view`
/// under the editor's own title bar.
fn secondary_window<V: Render>(
    title: String,
    content: (f32, f32),
    cx: &mut App,
    view: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> Option<WindowHandle<Root>> {
    let options = WindowOptions {
        app_id: Some(crate::APP_ID.to_owned()),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(
                tokens::scaled_width(content.0),
                tokens::scaled_width(content.1),
            ),
            cx,
        ))),
        kind: WindowKind::Floating,
        is_resizable: true,
        window_min_size: Some(size(px(480.), px(360.))),
        app_owns_titlebar_drag: true,
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client),
        window_background: crate::theme::active().effects.window,
        ..Default::default()
    };
    cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| view(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .ok()
}
