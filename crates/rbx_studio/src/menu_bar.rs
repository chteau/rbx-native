//! The top menu bar: File / Edit / View / Plugins / Test / Window / Help,
//! Studio's own, mounted above the dock area
//! (see `Render for Shell`). The structure is GPUI's own OS-menu shape
//! (`Menu`/`MenuItem`/`OwnedMenu`), and each dropdown is `gpui_component`'s
//! stock [`PopupMenu`](gpui_kit::component::menu::PopupMenu), which already
//! owns a menu's own keyboard contract: Up/Down through the items, Enter to
//! activate, Escape to close.
//!
//! What this module owns, rather than the toolkit's ready-made `AppMenuBar`,
//! is the **bar**: see [`bar::MenuBar`]. That component keeps which title is
//! current behind a private field with no way in from outside, so the desktop
//! way *into* a menu bar (F10, or a bare Alt tap) could not be wired to it at
//! all, and a menu bar no keyboard can reach is a WCAG 2.1.1 failure however
//! good its internal navigation is.
//!
//! Every item that maps to something the editor can already do dispatches
//! the same [`gpui::Action`] its keyboard shortcut resolves to, straight into
//! `Shell`'s existing, already-tested handler, Save (`shell::save`),
//! Undo/Redo (`shell::history`), Insert Part/Folder/Script/LocalScript/
//! ModuleScript and Delete (`shell::keys`), Copy/Paste/Duplicate
//! (`shell::clipboard`), Group/Ungroup (`shell::group`), through one global
//! `App::on_action` registration per action (see [`actions::install`]). A
//! menu click never focuses anything first, so these are global rather than
//! wired into the element tree: the same requirement `Shell::handle_shell_key`
//! already has for Ctrl+S/Z/Y/G/C/V/D (a command must fire no matter what
//! currently has focus).
//!
//! Anything this editor cannot do yet stays a visibly disabled item
//! ([`MenuPlaceholder`]) rather than a click that silently does nothing
//! while looking live. Style Editor is a Window item because Roblox puts
//! that panel under `Window` ⟩ UI (`studio/ui-overview.md`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::script_templates::Template;
use crate::shell::Shell;
use crate::tokens;

mod actions;
mod alt_tap;
mod bar;
mod menus;
#[cfg(target_os = "macos")]
mod native;
mod popup;

pub(crate) use bar::{install_key_bindings, MenuBar};
pub(crate) use menus::menus;

actions!(
    menu_bar,
    [
        MenuNewPlace,
        MenuOpenPlace,
        MenuOpenGame,
        MenuFindInGame,
        MenuSyncShared,
        MenuSaveAll,
        MenuPublishAll,
        MenuMarkShared,
        MenuUnmarkShared,
        MenuInsertObject,
        MenuToggleCommandBar,
        MenuSave,
        MenuSaveToFile,
        MenuClosePlace,
        MenuExportGltf,
        MenuExportFbx,
        MenuExportProject,
        MenuExportClean,
        MenuSaveAsTemplate,
        MenuSaveToRoblox,
        MenuPublishToRoblox,
        MenuLinkRobloxPlace,
        MenuVersionHistory,
        MenuMergePlace,
        MenuGameSettings,
        MenuDataStores,
        MenuUpdatePlanning,
        MenuOpenAutoSaves,
        MenuStudioSettings,
        /// Help › About RbxNative, and macOS's app menu item of that name.
        MenuAbout,
        /// Cmd+Q on macOS: the close button's question, then the whole app.
        MenuQuit,
        MenuCustomizeShortcuts,
        MenuBetaFeatures,
        MenuConnectAi,
        MenuScriptTemplates,
        MenuUndo,
        MenuRedo,
        MenuInsertPart,
        MenuInsertFolder,
        MenuImportModel,
        MenuInsertMesh,
        MenuInsertRig,
        MenuImportImage,
        MenuImportVideo,
        MenuInsertScript,
        MenuInsertLocalScript,
        MenuInsertModuleScript,
        MenuInsertModuleScriptClass,
        MenuDeleteInstance,
        MenuCutInstance,
        MenuCopyInstance,
        MenuPasteInstance,
        MenuPasteIntoInstance,
        MenuPasteAtOriginal,
        MenuPasteIntoAtOriginal,
        MenuDuplicateInstance,
        MenuGroup,
        MenuUngroup,
        MenuStyleEditor,
        MenuReduceMotion,
        MenuLargeTargets,
        MenuResetLayout,
        MenuTopView,
        MenuFrontView,
        MenuRightView,
        MenuCommandPalette,
        MenuQuickOpen,
        /// One per dock, so the View menu can put back one that has been
        /// closed, the only way back, which is why they are real actions
        /// rather than the placeholders they used to be.
        MenuToggleExplorer,
        MenuToggleProperties,
        MenuToggleOutput,
        MenuToggleViewport,
        MenuToggleCollaborators,
        MenuToggleInputActions,
        MenuToggleToolbox,
        MenuToggleSecurityAudit,
        MenuTogglePrePublish,
        MenuRequireGraph,
        MenuRunLocal,
        MenuTogglePlaytests,
        MenuToggleLayers,
        MenuToggleLoren,
        MenuToggleAvatarSettings,
        MenuToggleTerrainEditor,
        MenuToggleCallStack,
        MenuToggleWatch,
        MenuToggleLocalization,
        MenuToggleMicroProfiler,
        MenuToggleFindInPlace,
        MenuToggleScriptAnalysis,
        MenuToggleRemoteInspector,
        MenuToggleLeakFinder,
        MenuToggleLoadTest,
        MenuZoomIn,
        MenuZoomOut,
        MenuShowUi,
        MenuFullbright,
        MenuManagePlugins,
        MenuPluginsFolder,
        MenuPopularPlugins,
        MenuReloadPlugins,
        MenuPlay,
        MenuStop,
        MenuControllerEmulator,
        MenuDocumentation,
        MenuDeveloperForum,
        /// Shared by every item below that has no real handler yet; always
        /// paired with `.disabled(true)` (see `menus`), so `PopupMenu` never
        /// lets a click reach it, `actions::install` still gives it a no-op
        /// handler as a defensive backstop, never a crash.
        MenuPlaceholder,
    ]
);

/// Model's entry for one of the user's own script templates (see
/// `crate::script_templates`), by its place in `ScriptTemplates::extras`:
/// the one menu item whose action carries data, because the list is the
/// user's and is only known at startup.
#[derive(Clone, PartialEq, Eq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = menu_bar, no_json)]
pub(crate) struct MenuInsertTemplate {
    pub(crate) index: usize,
}

/// Builds the menu bar and registers its `Action` handlers against `shell`.
/// Returns the entity `Shell` holds and mounts as the first child of its
/// render tree (see [`MenuBar::bar`]).
pub(crate) fn build(shell: Entity<Shell>, templates: &[Template], cx: &mut App) -> Entity<MenuBar> {
    actions::install(&shell, cx);
    #[cfg(target_os = "macos")]
    native::set(templates, cx);
    MenuBar::new(menus(templates), cx)
}

/// The commands that work before any editor exists: Quit, and on macOS the
/// system menu bar. Once, at launch.
pub(crate) fn install_app(cx: &mut App) {
    actions::install_app(cx);
    #[cfg(target_os = "macos")]
    native::install(cx);
}

pub(crate) use actions::{retarget, untarget};

/// Rebuilds the menus after the user's templates changed on disk: Model's
/// template items carry an index into the list, so the old ones would now
/// point at the wrong template.
pub(crate) fn refresh(bar: &Entity<MenuBar>, templates: &[Template], cx: &mut App) {
    bar.update(cx, |bar, cx| bar.set_menus(menus(templates), cx));
    #[cfg(target_os = "macos")]
    native::set(templates, cx);
}

/// The system menu bar again after the network came or went: its items
/// that need Roblox are greyed out offline (the in-window bar reads the
/// status each time a menu opens).
pub(crate) fn follow_network(templates: &[Template], cx: &mut App) {
    #[cfg(target_os = "macos")]
    native::set(templates, cx);
    #[cfg(not(target_os = "macos"))]
    let _ = (templates, cx);
}

/// The bar as it sits under the title bar: the frame's own strip, painted
/// the same ground tone as the title bar above it, so a hairline is what
/// separates the two rather than a tone neither of them actually has.
///
/// The bar's own `size_full()` needs a definite height to fill, or it either
/// collapses to nothing or grows to cover the rows below it in a flex column.
pub(crate) fn bar(menu_bar: &Entity<MenuBar>) -> impl IntoElement {
    div()
        .w_full()
        .h(tokens::menu_bar_height())
        .flex_none()
        .when_some(tokens::on_ground(tokens::menu_bar()), |this, bg| {
            this.bg(bg)
        })
        .border_b(px(1.))
        .border_color(tokens::border())
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .child(menu_bar.clone())
}

#[cfg(test)]
#[path = "menu_bar/tests.rs"]
mod tests;
