//! The top menu bar: File / Edit / Model / View, mounted above the dock area
//! (see `Render for Shell`). The structure is GPUI's own OS-menu shape
//! (`Menu`/`MenuItem`/`OwnedMenu`), and each dropdown is `gpui_component`'s
//! stock [`PopupMenu`](gpui_kit::component::menu::PopupMenu), which already
//! owns a menu's own keyboard contract: Up/Down through the items, Enter to
//! activate, Escape to close.
//!
//! What this module owns, rather than the toolkit's ready-made `AppMenuBar`,
//! is the **bar** — see [`bar::MenuBar`]. That component keeps which title is
//! current behind a private field with no way in from outside, so the desktop
//! way *into* a menu bar (F10, or a bare Alt tap) could not be wired to it at
//! all, and a menu bar no keyboard can reach is a WCAG 2.1.1 failure however
//! good its internal navigation is.
//!
//! Every item that maps to something the editor can already do dispatches
//! the same [`gpui::Action`] its keyboard shortcut resolves to, straight into
//! `Shell`'s existing, already-tested handler — Save (`shell::save`),
//! Undo/Redo (`shell::history`), Insert Part/Folder/Script/LocalScript/
//! ModuleScript and Delete (`shell::keys`), Copy/Paste/Duplicate
//! (`shell::clipboard`), Group/Ungroup (`shell::group`) — through one global
//! `App::on_action` registration per action (see [`actions::install`]). A
//! menu click never focuses anything first, so these are global rather than
//! wired into the element tree: the same requirement `Shell::handle_shell_key`
//! already has for Ctrl+S/Z/Y/G/C/V/D (a command must fire no matter what
//! currently has focus).
//!
//! Everything this editor cannot do yet — New, Open…, Save As…, Publish to
//! Roblox…, Cut, Insert Object… — stays a visibly disabled item rather than
//! a click that silently does nothing while looking live. The one live View
//! item is Style Editor: Roblox puts that panel under `Window` ⟩ UI
//! (`studio/ui-overview.md`), and this editor's menus are File/Edit/Model/View,
//! so it goes under View.

use gpui_kit::*;

use crate::shell::Shell;
use crate::tokens;

mod actions;
mod alt_tap;
mod bar;
mod popup;

pub(crate) use bar::{install_key_bindings, MenuBar};

actions!(
    menu_bar,
    [
        MenuSave,
        MenuUndo,
        MenuRedo,
        MenuInsertPart,
        MenuInsertFolder,
        MenuInsertScript,
        MenuInsertLocalScript,
        MenuInsertModuleScript,
        MenuInsertModuleScriptClass,
        MenuDeleteInstance,
        MenuCutInstance,
        MenuCopyInstance,
        MenuPasteInstance,
        MenuPasteIntoInstance,
        MenuDuplicateInstance,
        MenuGroup,
        MenuUngroup,
        MenuStyleEditor,
        MenuReduceMotion,
        MenuLargeTargets,
        MenuResetLayout,
        /// One per dock, so the View menu can put back one that has been
        /// closed — the only way back, which is why they are real actions
        /// rather than the placeholders they used to be.
        MenuToggleExplorer,
        MenuToggleProperties,
        MenuToggleOutput,
        /// Shared by every item below that has no real handler yet; always
        /// paired with `.disabled(true)` (see `menus`), so `PopupMenu` never
        /// lets a click reach it — `actions::install` still gives it a no-op
        /// handler as a defensive backstop, never a crash.
        MenuPlaceholder,
    ]
);

/// Builds the menu bar and registers its `Action` handlers against `shell`.
/// Returns the entity `Shell` holds and mounts as the first child of its
/// render tree (see [`MenuBar::bar`]).
pub(crate) fn build(shell: Entity<Shell>, cx: &mut App) -> Entity<MenuBar> {
    actions::install(shell, cx);
    MenuBar::new(menus(), cx)
}

/// The menu structure itself. File and Edit hold this editor's real
/// commands (see this module's doc comment); Model holds the two quick
/// inserts `shell::keys` already binds to Ctrl+Shift+P/F, plus the three
/// script classes and the OOP `ModuleScript` variant — Roblox itself offers
/// `Script`/`LocalScript`/`ModuleScript` as distinct inserts, so this matches
/// that rather than collapsing them into one generic "Insert Script" — and
/// Group/Ungroup (`shell::group`, Ctrl+G/Ctrl+Shift+G). View's Explorer,
/// Properties and Command Bar items stay placeholders — those panels have no
/// show/hide command to wire them to yet — while Style Editor brings its own
/// Style Editor document to the front (see `shell::style_panel`).
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
                MenuItem::action("Cut", MenuCutInstance),
                MenuItem::action("Copy", MenuCopyInstance),
                MenuItem::action("Paste", MenuPasteInstance),
                MenuItem::action("Paste Into", MenuPasteIntoInstance),
                MenuItem::action("Duplicate", MenuDuplicateInstance),
                MenuItem::separator(),
                MenuItem::action("Delete", MenuDeleteInstance),
            ])
            .owned(),
        Menu::new("Model")
            .items(vec![
                MenuItem::action("Insert Part", MenuInsertPart),
                MenuItem::action("Insert Folder", MenuInsertFolder),
                MenuItem::separator(),
                MenuItem::action("Insert Script", MenuInsertScript),
                MenuItem::action("Insert LocalScript", MenuInsertLocalScript),
                MenuItem::action("Insert ModuleScript", MenuInsertModuleScript),
                MenuItem::action("Insert ModuleScript (Class)", MenuInsertModuleScriptClass),
                MenuItem::separator(),
                MenuItem::action("Insert Object…", MenuPlaceholder).disabled(true),
                MenuItem::separator(),
                MenuItem::action("Group", MenuGroup),
                MenuItem::action("Ungroup", MenuUngroup),
            ])
            .owned(),
        Menu::new("View")
            .items(vec![
                MenuItem::action("Explorer", MenuToggleExplorer),
                MenuItem::action("Properties", MenuToggleProperties),
                MenuItem::action("Output", MenuToggleOutput),
                MenuItem::action("Command Bar", MenuPlaceholder).disabled(true),
                MenuItem::separator(),
                MenuItem::action("Style Editor", MenuStyleEditor),
                MenuItem::separator(),
                // The accessibility settings the reference guidance calls
                // for a native app to expose itself rather than inherit
                // silently. The UI scale is here too, as its shortcuts.
                MenuItem::action("Reduce Motion", MenuReduceMotion),
                MenuItem::action("Large Click Targets", MenuLargeTargets),
                MenuItem::separator(),
                MenuItem::action("Reset Layout", MenuResetLayout),
            ])
            .owned(),
    ]
}

/// The bar as it sits under the title bar: the frame's own strip, and the one
/// surface in the whole design lighter than its neighbours — which is what
/// separates it from the black above it without a border.
///
/// The bar's own `size_full()` needs a definite height to fill, or it either
/// collapses to nothing or grows to cover the rows below it in a flex column.
pub(crate) fn bar(menu_bar: &Entity<MenuBar>) -> impl IntoElement {
    div()
        .w_full()
        .h(tokens::menu_bar_height())
        .flex_none()
        .bg(tokens::menu_bar())
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .child(menu_bar.clone())
}

#[cfg(test)]
#[path = "menu_bar/tests.rs"]
mod tests;
