//! One title's dropdown, built from the `OwnedMenu` items `menu_bar::menus`
//! declares.
//!
//! The toolkit builds the same thing from the same structures internally, but
//! only for its own `AppMenuBar` — the entry point is `pub(super)` to it — so
//! this is that translation, written out against `PopupMenu`'s public
//! builders. Nothing here is a behaviour change from what the toolkit did:
//! an action item keeps its checked and disabled state, a separator stays a
//! separator, and the component still owns everything below that.

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

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
