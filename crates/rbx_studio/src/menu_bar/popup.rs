//! One title's dropdown, built from the `OwnedMenu` items `menu_bar::menus`
//! declares.
//!
//! The toolkit builds the same thing from the same structures internally, but
//! only for its own `AppMenuBar` — the entry point is `pub(super)` to it — so
//! this is that translation, written out against `PopupMenu`'s public
//! builders. Nothing here is a behaviour change from what the toolkit did:
//! an action item keeps its checked and disabled state, a separator stays a
//! separator, and the component still owns everything below that.

use gpui_kit::component::h_flex;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

/// The rows that commit or destroy. The toolkit sizes an ordinary item at
/// 26px and offers no per-item height, but its `ElementItem` takes any
/// element and keeps the action, the disabled state and keyboard
/// navigation, so these rows are drawn as elements 44px tall (WCAG 2.5.5)
/// with the shortcut hint rendered the way the toolkit's own rows do.
pub(super) const PRIMARY_ROWS: [&str; 4] = [
    "Save to File",
    "Save to Roblox",
    "Publish to Roblox",
    "Delete",
];

/// `action_context` is where an item's action dispatches from — the handle
/// focus was on before the bar took it, so a command reads the same world it
/// would have if its shortcut had been typed instead.
pub(super) fn dropdown(
    items: &[OwnedMenuItem],
    action_context: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let items = items.to_vec();
    PopupMenu::build(window, cx, move |menu, _, _| {
        let menu = items.iter().fold(menu, |menu, item| match item {
            OwnedMenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } if PRIMARY_ROWS.contains(&name.as_ref()) => {
                let (label, shortcut_action) = (name.clone(), action.boxed_clone());
                let context = action_context.clone();
                menu.item(
                    PopupMenuItem::element(move |window, _| {
                        let hint = context
                            .as_ref()
                            .and_then(|handle| {
                                Kbd::binding_for_action_in(shortcut_action.as_ref(), handle, window)
                            })
                            .or_else(|| {
                                Kbd::binding_for_action(shortcut_action.as_ref(), None, window)
                            });
                        h_flex()
                            .debug_selector(|| format!("menu-row-{label}"))
                            .w_full()
                            .min_h(tokens::primary_target())
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(label.clone())
                            .children(
                                hint.map(|kbd| kbd.p_0().border_0().bg(gpui::transparent_white())),
                            )
                    })
                    .action(action.boxed_clone())
                    .checked(*checked)
                    .disabled(*disabled),
                )
            }
            OwnedMenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } => menu.menu_with_check_and_disabled(
                name.clone(),
                *checked,
                action.boxed_clone(),
                *disabled,
            ),
            OwnedMenuItem::Separator => menu.separator(),
            // `menus()` builds neither, and there is nothing sensible to draw
            // for an OS-managed menu on this platform.
            OwnedMenuItem::Submenu(_) | OwnedMenuItem::SystemMenu(_) => menu,
        });
        menu.when_some(action_context, |menu, context| menu.action_context(context))
    })
}
