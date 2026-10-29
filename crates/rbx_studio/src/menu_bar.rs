//! The top menu bar: File / Edit / Model / View, mounted above the dock area
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
//! while looking live. Style Editor is a View item because Roblox puts that
//! panel under `Window` ⟩ UI
//! (`studio/ui-overview.md`), and this editor's menus are File/Edit/Model/View,
//! so it goes under View.

use gpui_kit::*;

use crate::script_templates::Template;
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
        MenuExportProject,
        MenuExportClean,
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
        MenuCustomizeShortcuts,
        MenuBetaFeatures,
        MenuScriptTemplates,
        MenuUndo,
        MenuRedo,
        MenuInsertPart,
        MenuInsertFolder,
        MenuImportModel,
        MenuInsertMesh,
        MenuInsertRig,
        MenuImportImage,
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
        MenuToggleSecurityAudit,
        MenuRequireGraph,
        MenuTogglePlaytests,
        MenuToggleLayers,
        MenuToggleLoren,
        MenuToggleAvatarSettings,
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
    MenuBar::new(menus(templates), cx)
}

pub(crate) use actions::retarget;

/// Rebuilds the menus after the user's templates changed on disk: Model's
/// template items carry an index into the list, so the old ones would now
/// point at the wrong template.
pub(crate) fn refresh(bar: &Entity<MenuBar>, templates: &[Template], cx: &mut App) {
    bar.update(cx, |bar, cx| bar.set_menus(menus(templates), cx));
}

/// The menu structure itself. File and Edit hold this editor's real
/// commands (see this module's doc comment); Model holds the two quick
/// inserts `shell::keys` handles (Insert Folder on Ctrl+Shift+F), plus the three
/// script classes and the OOP `ModuleScript` variant, Roblox itself offers
/// `Script`/`LocalScript`/`ModuleScript` as distinct inserts, so this matches
/// that rather than collapsing them into one generic "Insert Script", and
/// Group/Ungroup (`shell::group`, Ctrl+G/Ctrl+Shift+G). View's Explorer,
/// Properties and Command Bar items show and hide their panels, and Style
/// Editor brings its own
/// Style Editor document to the front (see `shell::style_panel`). The
/// user's own script templates close Model, after a separator, named the way
/// the ribbon's Script menu names them, so the two menus list the same
/// things; last because the list is the user's and can run long, and the
/// popup does not scroll, so it must not push Group/Ungroup out of reach.
///
/// Also the command palette's source (`shell::palette`): every enabled item
/// here is a palette row, so the two cannot list different commands.
pub(crate) fn menus(templates: &[Template]) -> Vec<OwnedMenu> {
    let mut model = vec![
        MenuItem::action("Insert Part", MenuInsertPart),
        MenuItem::action("Insert Folder", MenuInsertFolder),
        MenuItem::separator(),
        MenuItem::action("Insert Script", MenuInsertScript),
        MenuItem::action("Insert LocalScript", MenuInsertLocalScript),
        MenuItem::action("Insert ModuleScript", MenuInsertModuleScript),
        MenuItem::action("Insert ModuleScript (Class)", MenuInsertModuleScriptClass),
        MenuItem::separator(),
        MenuItem::action("Insert Object…", MenuInsertObject),
        MenuItem::action("Insert Mesh…", MenuInsertMesh),
        MenuItem::action("Insert Rig…", MenuInsertRig),
        MenuItem::action("Import 3D Model…", MenuImportModel),
        MenuItem::action("Import Image…", MenuImportImage),
        MenuItem::separator(),
        // The id Sync Shared matches across a game's places.
        MenuItem::action("Mark as Shared", MenuMarkShared),
        MenuItem::action("Unmark Shared", MenuUnmarkShared),
        MenuItem::separator(),
        MenuItem::action("Group", MenuGroup),
        MenuItem::action("Ungroup", MenuUngroup),
    ];
    if !templates.is_empty() {
        model.push(MenuItem::separator());
    }
    model.extend(templates.iter().enumerate().map(|(index, template)| {
        MenuItem::action(
            format!("Insert {} ({})", template.name, template.class),
            MenuInsertTemplate { index },
        )
    }));

    let mut file = vec![
        MenuItem::action("New", MenuNewPlace),
        MenuItem::action("Open…", MenuOpenPlace),
        // Studio's place for it, under Open. Back to Home; asks
        // first when there are unsaved changes (`shell::close_place`).
        MenuItem::action("Close Place", MenuClosePlace),
        // Every place of a game in one window (`shell::game`).
        MenuItem::action("Open Game…", MenuOpenGame),
        MenuItem::separator(),
        // Studio's names: Save to File writes back to the file
        // being edited (Ctrl+S), Save to File As… asks for a new one
        // and switches to it (see `shell::save`).
        MenuItem::action("Save to File", MenuSave),
        MenuItem::action("Save to File As…", MenuSaveToFile),
        MenuItem::action("Export as glTF…", MenuExportGltf),
        // A folder Argon or Rojo syncs from; File › Open reads one
        // back (a `*.project.json`).
        MenuItem::action("Export as Project…", MenuExportProject),
        // A copy without layers and 3D notes (`crate::strip`).
        MenuItem::action("Export Clean Copy…", MenuExportClean),
        MenuItem::separator(),
        // A game window's own: every place at once.
        MenuItem::action("Find in Game…", MenuFindInGame),
        MenuItem::action("Sync Shared…", MenuSyncShared),
        MenuItem::action("Save All", MenuSaveAll),
        MenuItem::action("Publish All…", MenuPublishAll),
        MenuItem::separator(),
        // Uploads, not local saves: see `shell::roblox_publish`. An
        // unlinked file asks for its place on first use.
        MenuItem::action("Save to Roblox", MenuSaveToRoblox),
        MenuItem::action("Publish to Roblox", MenuPublishToRoblox),
        MenuItem::action("Link to Roblox Place…", MenuLinkRobloxPlace),
        // Studio's lives under Window; this bar has none, and it
        // belongs with the uploads it browses and restores.
        MenuItem::action("Version History…", MenuVersionHistory),
        MenuItem::action("Merge Place\u{2026}", MenuMergePlace),
        // Studio's own File › Experience Settings; Home has it too.
        MenuItem::action("Game Settings…", MenuGameSettings),
        // Creator Hub keeps these beside the experience's settings.
        MenuItem::action("Data Stores…", MenuDataStores),
        MenuItem::separator(),
        // Studio files this under File › Advanced; this bar draws
        // no submenus (`popup`), so it sits flat, under Studio's
        // own name for it.
        MenuItem::action("Open Auto Saves", MenuOpenAutoSaves),
        MenuItem::separator(),
        MenuItem::action("Studio Settings…", MenuStudioSettings),
        // Studio keeps both of these under File as well.
        MenuItem::action("Customize Shortcuts…", MenuCustomizeShortcuts),
        MenuItem::action("Beta Features…", MenuBetaFeatures),
        MenuItem::action("Script Templates…", MenuScriptTemplates),
    ];
    if crate::beta::enabled(crate::update_planning::BETA_ID) {
        // Experience-level, so beside Game Settings and Data Stores.
        let at = file.iter().position(
            |item| matches!(item, MenuItem::Action { name, .. } if name.as_ref() == "Data Stores…"),
        );
        file.insert(
            at.map_or(file.len(), |at| at + 1),
            MenuItem::action("Update Planning…", MenuUpdatePlanning),
        );
    }
    vec![
        Menu::new("File").items(file).owned(),
        Menu::new("Edit")
            .items(vec![
                MenuItem::action("Undo", MenuUndo),
                MenuItem::action("Redo", MenuRedo),
                MenuItem::separator(),
                MenuItem::action("Cut", MenuCutInstance),
                MenuItem::action("Copy", MenuCopyInstance),
                MenuItem::action("Paste", MenuPasteInstance),
                MenuItem::action("Paste Into", MenuPasteIntoInstance),
                // Studio's own pair: the same pastes without lifting the
                // copy off whatever it lands inside.
                MenuItem::action("Paste at Original Location", MenuPasteAtOriginal),
                MenuItem::action("Paste into at Original Location", MenuPasteIntoAtOriginal),
                MenuItem::action("Duplicate", MenuDuplicateInstance),
                MenuItem::separator(),
                MenuItem::action("Delete", MenuDeleteInstance),
            ])
            .owned(),
        Menu::new("Model").items(model).owned(),
        Menu::new("View")
            .items(
                vec![
                    MenuItem::action("Explorer", MenuToggleExplorer),
                    MenuItem::action("Properties", MenuToggleProperties),
                    MenuItem::action("Output", MenuToggleOutput),
                    MenuItem::action("Viewport", MenuToggleViewport),
                    MenuItem::action("Command Bar", MenuToggleCommandBar),
                    MenuItem::separator(),
                    // Fixed camera views, the same snap a click on the
                    // orientation indicator's cube does.
                    MenuItem::action("Top View", MenuTopView),
                    MenuItem::action("Front View", MenuFrontView),
                    MenuItem::action("Right View", MenuRightView),
                    MenuItem::separator(),
                    MenuItem::action("Style Editor", MenuStyleEditor),
                    // Team presence: who else is in the place, from Studio or
                    // rbx-native. Shut by default, so it needs a way in.
                    MenuItem::action("Collaborators", MenuToggleCollaborators),
                    MenuItem::action("Input Actions", MenuToggleInputActions),
                    MenuItem::action("Security Audit", MenuToggleSecurityAudit),
                ]
                .into_iter()
                // A window rather than a dock, listed with the docks that read
                // the place's scripts. Behind its beta switch.
                .chain(
                    crate::beta::enabled(crate::shell::REQUIRE_GRAPH_BETA_ID)
                        .then(|| MenuItem::action("Require Graph", MenuRequireGraph)),
                )
                .chain(vec![
                    MenuItem::action("Playtests", MenuTogglePlaytests),
                    MenuItem::action("Layers", MenuToggleLayers),
                    MenuItem::action("Loren", MenuToggleLoren),
                    MenuItem::action("Avatar Settings", MenuToggleAvatarSettings),
                    MenuItem::separator(),
                    // The accessibility settings the reference guidance calls
                    // for a native app to expose itself rather than inherit
                    // silently. The UI scale is here too, as its shortcuts.
                    MenuItem::action("Reduce Motion", MenuReduceMotion),
                    MenuItem::action("Large Click Targets", MenuLargeTargets),
                    MenuItem::separator(),
                    MenuItem::action("Reset Layout", MenuResetLayout),
                    // The palette is a keyboard tool, but a shortcut nobody can
                    // find is recall, not recognition: the menu is where its
                    // name and its keys are on show.
                    MenuItem::action("Quick Open…", MenuQuickOpen),
                    MenuItem::action("Command Palette…", MenuCommandPalette),
                ])
                .collect::<Vec<_>>(),
            )
            .owned(),
    ]
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
        .bg(tokens::menu_bar())
        .border_b(px(1.))
        .border_color(tokens::border())
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .child(menu_bar.clone())
}

#[cfg(test)]
#[path = "menu_bar/tests.rs"]
mod tests;
