//! What each menu holds, in Studio 0.748's order (measured in Studio, menu
//! by menu).

use gpui_kit::{Menu, MenuItem, OwnedMenu};

use super::*;
use crate::script_templates::Template;

/// The menu structure itself: Studio 0.748's bar, File / Edit / View /
/// Plugins / Test / Window / Help, each item where Studio files it and under
/// Studio's name, this editor's own commands beside their nearest Studio
/// neighbour. What Studio keeps in a submenu sits flat here, between
/// separators, in the submenu's order: this bar draws no submenus
/// (`popup`).
///
/// Studio has no Model menu; the inserts it held (Part, Folder, the three
/// script classes and the OOP `ModuleScript`, Mesh, Rig) close Edit, beside
/// Group/Ungroup, then the user's own script templates, named the way the
/// ribbon's Script menu names them; last because that list is the user's
/// and can run long. Imports are File's, as Studio's Import is, and Insert
/// Object is Window's. About stays in Help: on macOS the app menu already
/// carries it, and Studio's File copy would show it twice there.
///
/// Also the command palette's source (`shell::palette`): every enabled item
/// here is a palette row, so the two cannot list different commands.
pub(crate) fn menus(templates: &[Template]) -> Vec<OwnedMenu> {
    menus_with(templates, Betas::current())
}

/// The beta switches that add menu items.
#[derive(Clone, Copy, Default)]
pub(super) struct Betas {
    pub(super) update_planning: bool,
    pub(super) local_run: bool,
    pub(super) pseudo_loc: bool,
    pub(super) require_graph: bool,
}

impl Betas {
    fn current() -> Self {
        Betas {
            update_planning: crate::beta::enabled(crate::update_planning::BETA_ID),
            local_run: crate::beta::enabled(crate::shell::LOCAL_RUN_BETA_ID),
            pseudo_loc: crate::beta::enabled(crate::shell::PSEUDO_LOC_BETA_ID),
            require_graph: crate::beta::enabled(crate::shell::REQUIRE_GRAPH_BETA_ID),
        }
    }
}

pub(super) fn menus_with(templates: &[Template], betas: Betas) -> Vec<OwnedMenu> {
    vec![
        Menu::new("File").items(file_menu(betas)).owned(),
        Menu::new("Edit").items(edit_menu(templates)).owned(),
        Menu::new("View").items(view_menu()).owned(),
        Menu::new("Plugins")
            .items(vec![
                MenuItem::action("Manage Plugins", MenuManagePlugins),
                MenuItem::action("Plugins Folder", MenuPluginsFolder),
                MenuItem::separator(),
                MenuItem::action("Popular Plugins\u{2026}", MenuPopularPlugins),
                MenuItem::action("Reload Plugins", MenuReloadPlugins),
            ])
            .owned(),
        Menu::new("Test").items(test_menu(betas)).owned(),
        Menu::new("Window").items(window_menu(betas)).owned(),
        Menu::new("Help")
            .items(vec![
                MenuItem::action("Documentation", MenuDocumentation),
                MenuItem::action("Developer Forum", MenuDeveloperForum),
                MenuItem::separator(),
                MenuItem::action("About RbxNative", MenuAbout),
            ])
            .owned(),
    ]
}

/// `item` only when `on`, for a beta's entries.
fn gated(on: bool, item: MenuItem) -> Option<MenuItem> {
    on.then_some(item)
}

fn file_menu(betas: Betas) -> Vec<MenuItem> {
    [
        Some(MenuItem::action("New", MenuNewPlace)),
        Some(MenuItem::action("Open from File\u{2026}", MenuOpenPlace)),
        // Every place of a game in one window (`shell::game`).
        Some(MenuItem::action("Open Game\u{2026}", MenuOpenGame)),
        Some(MenuItem::separator()),
        // Back to Home; asks first when there are unsaved changes
        // (`shell::close_place`).
        Some(MenuItem::action("Close Place", MenuClosePlace)),
        Some(MenuItem::separator()),
        // Studio's Import is the 3D importer; the image and video uploads
        // sit under it.
        Some(MenuItem::action("Import 3D Model\u{2026}", MenuImportModel)),
        Some(MenuItem::action("Import Image\u{2026}", MenuImportImage)),
        crate::shell::video_upload_enabled()
            .then(|| MenuItem::action("Import Video\u{2026}", MenuImportVideo)),
        Some(MenuItem::action("Export as glTF\u{2026}", MenuExportGltf)),
        Some(MenuItem::action("Export as FBX\u{2026}", MenuExportFbx)),
        // A folder Argon or Rojo syncs from; Open from File reads one back
        // (a `*.project.json`).
        Some(MenuItem::action(
            "Export as Project\u{2026}",
            MenuExportProject,
        )),
        // A copy without layers and 3D notes (`crate::strip`).
        Some(MenuItem::action(
            "Export Clean Copy\u{2026}",
            MenuExportClean,
        )),
        Some(MenuItem::separator()),
        // Save to File writes back to the file being edited (Ctrl+S), Save
        // to File As… asks for a new one and switches to it (`shell::save`).
        Some(MenuItem::action("Save to File", MenuSave)),
        Some(MenuItem::action("Save to File As\u{2026}", MenuSaveToFile)),
        Some(MenuItem::action(
            "Save as Template\u{2026}",
            MenuSaveAsTemplate,
        )),
        // Uploads, not local saves: see `shell::roblox_publish`. An
        // unlinked file asks for its place on first use.
        Some(MenuItem::action("Save to Roblox", MenuSaveToRoblox)),
        Some(MenuItem::separator()),
        Some(MenuItem::action("Publish", MenuPublishToRoblox)),
        Some(MenuItem::action(
            "Link to Roblox Place\u{2026}",
            MenuLinkRobloxPlace,
        )),
        // Studio's lives under Window; it belongs with the uploads it
        // browses and restores.
        Some(MenuItem::action(
            "Version History\u{2026}",
            MenuVersionHistory,
        )),
        Some(MenuItem::action("Merge Place\u{2026}", MenuMergePlace)),
        Some(MenuItem::separator()),
        // A game window's own: every place at once.
        Some(MenuItem::action("Find in Game\u{2026}", MenuFindInGame)),
        Some(MenuItem::action("Sync Shared\u{2026}", MenuSyncShared)),
        Some(MenuItem::action("Save All", MenuSaveAll)),
        Some(MenuItem::action("Publish All\u{2026}", MenuPublishAll)),
        Some(MenuItem::separator()),
        Some(MenuItem::action(
            "Experience Settings\u{2026}",
            MenuGameSettings,
        )),
        Some(MenuItem::action(
            "Avatar Settings",
            MenuToggleAvatarSettings,
        )),
        // Experience-level, like Creator Hub keeps them.
        Some(MenuItem::action("Data Stores\u{2026}", MenuDataStores)),
        gated(
            betas.update_planning,
            MenuItem::action("Update Planning\u{2026}", MenuUpdatePlanning),
        ),
        Some(MenuItem::separator()),
        Some(MenuItem::action(
            "Studio Settings\u{2026}",
            MenuStudioSettings,
        )),
        Some(MenuItem::action("Beta Features\u{2026}", MenuBetaFeatures)),
        Some(MenuItem::action(
            "Customize Shortcuts\u{2026}",
            MenuCustomizeShortcuts,
        )),
        Some(MenuItem::action(
            "Connect an AI Assistant\u{2026}",
            MenuConnectAi,
        )),
        Some(MenuItem::action(
            "Script Templates\u{2026}",
            MenuScriptTemplates,
        )),
        Some(MenuItem::separator()),
        // Studio files this under File › Advanced.
        Some(MenuItem::action("Open Auto Saves", MenuOpenAutoSaves)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn edit_menu(templates: &[Template]) -> Vec<MenuItem> {
    let mut edit = vec![
        MenuItem::action("Undo", MenuUndo),
        MenuItem::action("Redo", MenuRedo),
        MenuItem::separator(),
        MenuItem::action("Cut", MenuCutInstance),
        MenuItem::action("Copy", MenuCopyInstance),
        MenuItem::action("Paste", MenuPasteInstance),
        MenuItem::action("Paste Into", MenuPasteIntoInstance),
        // The same pastes without lifting the copy off whatever it lands
        // inside.
        MenuItem::action("Paste at Original Location", MenuPasteAtOriginal),
        MenuItem::action("Paste into at Original Location", MenuPasteIntoAtOriginal),
        MenuItem::action("Duplicate", MenuDuplicateInstance),
        MenuItem::action("Delete", MenuDeleteInstance),
        MenuItem::separator(),
        MenuItem::action("Group", MenuGroup),
        MenuItem::action("Ungroup", MenuUngroup),
        MenuItem::separator(),
        MenuItem::action("Insert Part", MenuInsertPart),
        MenuItem::action("Insert Folder", MenuInsertFolder),
        MenuItem::action("Insert Script", MenuInsertScript),
        MenuItem::action("Insert LocalScript", MenuInsertLocalScript),
        MenuItem::action("Insert ModuleScript", MenuInsertModuleScript),
        MenuItem::action("Insert ModuleScript (Class)", MenuInsertModuleScriptClass),
        MenuItem::action("Insert Mesh\u{2026}", MenuInsertMesh),
        MenuItem::action("Insert Rig\u{2026}", MenuInsertRig),
        MenuItem::separator(),
        // The id Sync Shared matches across a game's places.
        MenuItem::action("Mark as Shared", MenuMarkShared),
        MenuItem::action("Unmark Shared", MenuUnmarkShared),
    ];
    if !templates.is_empty() {
        edit.push(MenuItem::separator());
    }
    edit.extend(templates.iter().enumerate().map(|(index, template)| {
        MenuItem::action(
            format!("Insert {} ({})", template.name, template.class),
            MenuInsertTemplate { index },
        )
    }));
    edit
}

fn view_menu() -> Vec<MenuItem> {
    vec![
        // The UI scale, Studio's Zoom In / Zoom Out.
        MenuItem::action("Zoom In", MenuZoomIn),
        MenuItem::action("Zoom Out", MenuZoomOut),
        MenuItem::separator(),
        MenuItem::action("Show UI", MenuShowUi),
        MenuItem::action("Fullbright", MenuFullbright),
        MenuItem::separator(),
        // Fixed camera views, the same snap a click on the orientation
        // indicator's cube does.
        MenuItem::action("Top View", MenuTopView),
        MenuItem::action("Front View", MenuFrontView),
        MenuItem::action("Right View", MenuRightView),
        MenuItem::separator(),
        // The accessibility settings the reference guidance calls for a
        // native app to expose itself rather than inherit silently.
        MenuItem::action("Reduce Motion", MenuReduceMotion),
        MenuItem::action("Large Click Targets", MenuLargeTargets),
        MenuItem::separator(),
        // The palette is a keyboard tool, but a shortcut nobody can find is
        // recall, not recognition: the menu is where its name and its keys
        // are on show.
        MenuItem::action("Quick Open\u{2026}", MenuQuickOpen),
        MenuItem::action("Command Palette\u{2026}", MenuCommandPalette),
    ]
}

fn test_menu(betas: Betas) -> Vec<MenuItem> {
    [
        Some(MenuItem::action("Play", MenuPlay)),
        gated(
            betas.local_run,
            MenuItem::action("Run (local)", MenuRunLocal),
        ),
        Some(MenuItem::action("Stop", MenuStop)),
        Some(MenuItem::separator()),
        Some(MenuItem::action(
            "Controller Emulator",
            MenuControllerEmulator,
        )),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Studio's Window menu: its docks first, then its submenus' docks in
/// their order (3D, Collaboration, Debug, Input, Localization, Performance,
/// Script), then this editor's own docks, then Reset Layout.
fn window_menu(betas: Betas) -> Vec<MenuItem> {
    let Betas {
        local_run,
        pseudo_loc,
        require_graph,
        ..
    } = betas;
    [
        Some(MenuItem::action("Toolbox", MenuToggleToolbox)),
        Some(MenuItem::action("Properties", MenuToggleProperties)),
        Some(MenuItem::action("Explorer", MenuToggleExplorer)),
        Some(MenuItem::action("Output", MenuToggleOutput)),
        Some(MenuItem::action("Insert Object\u{2026}", MenuInsertObject)),
        Some(MenuItem::separator()),
        Some(MenuItem::action("Terrain Editor", MenuToggleTerrainEditor)),
        Some(MenuItem::action("Collaborators", MenuToggleCollaborators)),
        Some(MenuItem::action("Call Stack", MenuToggleCallStack)),
        Some(MenuItem::action("Watch", MenuToggleWatch)),
        Some(MenuItem::action("Input Actions", MenuToggleInputActions)),
        gated(
            pseudo_loc,
            MenuItem::action("Localization", MenuToggleLocalization),
        ),
        gated(
            local_run,
            MenuItem::action("MicroProfiler", MenuToggleMicroProfiler),
        ),
        Some(MenuItem::action("Command Bar", MenuToggleCommandBar)),
        Some(MenuItem::action("Find in Place", MenuToggleFindInPlace)),
        Some(MenuItem::action(
            "Script Analysis",
            MenuToggleScriptAnalysis,
        )),
        Some(MenuItem::separator()),
        Some(MenuItem::action("Viewport", MenuToggleViewport)),
        // Roblox lists it under UI in its overview docs; it's the UI tab's.
        Some(MenuItem::action("Style Editor", MenuStyleEditor)),
        Some(MenuItem::action("Layers", MenuToggleLayers)),
        Some(MenuItem::action("Loren", MenuToggleLoren)),
        Some(MenuItem::action("Security Audit", MenuToggleSecurityAudit)),
        Some(MenuItem::action("Pre-publish Check", MenuTogglePrePublish)),
        // A window rather than a dock, listed with the docks that read the
        // place's scripts. Behind its beta switch.
        gated(
            require_graph,
            MenuItem::action("Require Graph", MenuRequireGraph),
        ),
        Some(MenuItem::action("Playtests", MenuTogglePlaytests)),
        gated(
            local_run,
            MenuItem::action("Remote Inspector", MenuToggleRemoteInspector),
        ),
        gated(
            local_run,
            MenuItem::action("Leak Finder", MenuToggleLeakFinder),
        ),
        gated(local_run, MenuItem::action("Load Test", MenuToggleLoadTest)),
        Some(MenuItem::separator()),
        Some(MenuItem::action("Reset Layout", MenuResetLayout)),
    ]
    .into_iter()
    .flatten()
    .collect()
}
