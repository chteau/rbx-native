//! macOS's own menu bar: the app menu, then the in-window bar's menus, so
//! the system bar names this app instead of whichever one was in front
//! before. Every item dispatches the same action as its in-window twin.

use gpui_kit::component::input::SelectAll;
use gpui_kit::*;

use super::{menus, MenuQuit, MenuStudioSettings, Template};

actions!(menu_bar, [MenuAbout, MenuHide, MenuHideOthers, MenuShowAll]);

pub(super) fn install(cx: &mut App) {
    cx.on_action(|_: &MenuAbout, _| about());
    cx.on_action(|_: &MenuHide, cx| cx.hide());
    cx.on_action(|_: &MenuHideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &MenuShowAll, cx| cx.unhide_other_apps());
    cx.bind_keys([
        KeyBinding::new("cmd-h", MenuHide, None),
        KeyBinding::new("alt-cmd-h", MenuHideOthers, None),
    ]);
    set(&[], cx);
}

/// Rebuilt with the in-window bar, whose Model menu lists the user's
/// templates.
pub(super) fn set(templates: &[Template], cx: &App) {
    let app = Menu::new("RbxNative").items([
        MenuItem::action("About RbxNative", MenuAbout),
        MenuItem::separator(),
        MenuItem::action("Settings\u{2026}", MenuStudioSettings),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide RbxNative", MenuHide),
        MenuItem::action("Hide Others", MenuHideOthers),
        MenuItem::action("Show All", MenuShowAll),
        MenuItem::separator(),
        MenuItem::action("Quit RbxNative", MenuQuit),
    ]);
    cx.set_menus(std::iter::once(app).chain(menus(templates).into_iter().map(menu)));
}

fn menu(owned: OwnedMenu) -> Menu {
    let mut items: Vec<MenuItem> = owned.items.into_iter().map(item).collect();
    if owned.name.as_ref() == "Edit" {
        // The focused text field's; the place itself has no select-all.
        items.push(MenuItem::separator());
        items.push(MenuItem::action("Select All", SelectAll));
    }
    Menu::new(owned.name).items(items).disabled(owned.disabled)
}

fn item(owned: OwnedMenuItem) -> MenuItem {
    match owned {
        OwnedMenuItem::Separator => MenuItem::separator(),
        OwnedMenuItem::Submenu(owned) => MenuItem::submenu(menu(owned)),
        OwnedMenuItem::SystemMenu(owned) => MenuItem::os_submenu(owned.name, owned.menu_type),
        OwnedMenuItem::Action {
            name,
            action,
            os_action,
            checked,
            disabled,
        } => MenuItem::Action {
            name: name.into(),
            action,
            os_action,
            checked,
            disabled,
        },
    }
}

/// AppKit's standard About panel: the bundle's name, icon and version.
fn about() {
    if let Some(main) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(main).orderFrontStandardAboutPanel(None);
    }
}
