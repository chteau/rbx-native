//! The top menu bar: File / Edit / Model / View, mounted above the dock area
//! (see `Render for Shell`). Built on `gpui_component`'s [`AppMenuBar`] — the
//! ready-made Windows/Linux application menu bar (this app only targets
//! Linux) — rather than hand-rolled from `PopupMenu`s: it already owns the
//! open/close/hover/keyboard-nav dance a menu bar needs, driven by the same
//! OS-menu structures (`Menu`/`MenuItem`/`OwnedMenu`) GPUI defines for
//! exactly this.
//!
//! Every item that maps to something the editor can already do dispatches
//! the same [`gpui::Action`] its keyboard shortcut resolves to, straight into
//! `Shell`'s existing, already-tested handler — Save (`shell::save`),
//! Undo/Redo (`shell::history`), Insert Part/Folder and Delete
//! (`shell::keys`) — through one global `App::on_action` registration per
//! action (see [`install_actions`]). A menu click never focuses anything
//! first, so these are global rather than wired into the element tree: the
//! same requirement `Shell::handle_shell_key` already has for Ctrl+S/Z/Y (a
//! command must fire no matter what currently has focus).
//!
//! Everything this editor cannot do yet — New, Open…, Save As…, Publish to
//! Roblox…, Cut/Copy/Paste, Insert Object…, and the whole View menu — stays a
//! visibly disabled item rather than a click that silently does nothing
//! while looking live.

use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::{ActiveTheme, GlobalState};
use gpui_kit::*;

use crate::shell::Shell;

actions!(
    menu_bar,
    [
        MenuSave,
        MenuUndo,
        MenuRedo,
        MenuInsertPart,
        MenuInsertFolder,
        MenuDeleteInstance,
        /// Shared by every item below that has no real handler yet; always
        /// paired with `.disabled(true)` (see `menus`), so `PopupMenu` never
        /// lets a click reach it — `install_actions` still gives it a no-op
        /// handler as a defensive backstop, never a crash.
        MenuPlaceholder,
    ]
);

/// Builds the menu bar and registers its `Action` handlers against `shell`.
/// Returns the entity `Shell` holds and mounts as the first child of its
/// render tree (see [`bar`]).
pub(crate) fn build(shell: Entity<Shell>, cx: &mut App) -> Entity<AppMenuBar> {
    GlobalState::global_mut(cx).set_app_menus(menus());
    install_actions(shell, cx);
    AppMenuBar::new(cx)
}

/// The bar as it sits at the top of the window: a fixed height and a bottom
/// border, the same treatment the Command Bar gets at the other end of the
/// window (see `command_bar::CommandBar::render`) — `AppMenuBar`'s own
/// `size_full()` needs a definite height to fill, or it either collapses to
/// nothing or grows to cover the dock area below it in a flex column.
pub(crate) fn bar(menu_bar: &Entity<AppMenuBar>, cx: &App) -> impl IntoElement {
    div()
        .w_full()
        .h(px(32.))
        .flex_none()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(menu_bar.clone())
}

/// The menu structure itself. File and Edit hold this editor's real
/// commands (see this module's doc comment); Model holds the two quick
/// inserts `shell::keys` already binds to Ctrl+Shift+P/F. View is entirely
/// placeholder — there is no per-panel show/hide command to wire it to yet.
fn menus() -> Vec<OwnedMenu> {
    vec![
        Menu::new("File")
            .items(vec![
                MenuItem::action("New", MenuPlaceholder).disabled(true),
                MenuItem::action("Open…", MenuPlaceholder).disabled(true),
                MenuItem::separator(),
                MenuItem::action("Save", MenuSave),
                MenuItem::action("Save As…", MenuPlaceholder).disabled(true),
                MenuItem::separator(),
                MenuItem::action("Publish to Roblox…", MenuPlaceholder).disabled(true),
            ])
            .owned(),
        Menu::new("Edit")
            .items(vec![
                MenuItem::action("Undo", MenuUndo),
                MenuItem::action("Redo", MenuRedo),
                MenuItem::separator(),
                MenuItem::action("Cut", MenuPlaceholder).disabled(true),
                MenuItem::action("Copy", MenuPlaceholder).disabled(true),
                MenuItem::action("Paste", MenuPlaceholder).disabled(true),
                MenuItem::separator(),
                MenuItem::action("Delete", MenuDeleteInstance),
            ])
            .owned(),
        Menu::new("Model")
            .items(vec![
                MenuItem::action("Insert Part", MenuInsertPart),
                MenuItem::action("Insert Folder", MenuInsertFolder),
                MenuItem::separator(),
                MenuItem::action("Insert Object…", MenuPlaceholder).disabled(true),
            ])
            .owned(),
        Menu::new("View")
            .items(vec![
                MenuItem::action("Explorer", MenuPlaceholder).disabled(true),
                MenuItem::action("Properties", MenuPlaceholder).disabled(true),
                MenuItem::action("Command Bar", MenuPlaceholder).disabled(true),
            ])
            .owned(),
    ]
}

/// One `App::on_action` per wired command, plus a no-op for
/// [`MenuPlaceholder`] (see its own doc comment). Global rather than an
/// element-tree `on_action`, because `Context::on_action` may only be
/// registered during an entity's own paint pass — `build` runs once, from
/// `Shell::new`, well before `Shell` ever renders.
fn install_actions(shell: Entity<Shell>, cx: &mut App) {
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuSave, cx| {
            shell.update(cx, |shell, cx| shell.save(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuUndo, cx| {
            shell.update(cx, |shell, cx| shell.undo(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuRedo, cx| {
            shell.update(cx, |shell, cx| shell.redo(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertPart, cx| {
            shell.update(cx, |shell, cx| shell.insert_instance("Part", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertFolder, cx| {
            shell.update(cx, |shell, cx| shell.insert_instance("Folder", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuDeleteInstance, cx| {
            shell.update(cx, |shell, cx| shell.delete_selected(cx));
        }
    });
    cx.on_action(move |_: &MenuPlaceholder, _cx| {});
}
