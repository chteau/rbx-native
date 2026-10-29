//! One `App::on_action` per wired menu command, registered once, by the
//! first `menu_bar::build`, against whichever editor [`retarget`] last
//! named: the newest editor window, or the place a game window shows.
//!
//! Global rather than an element-tree `on_action`, because
//! `Context::on_action` may only be registered during an entity's own paint
//! pass, `build` runs once, from `Shell::new`, well before `Shell` ever
//! renders.

use gpui_kit::{App, Context, Entity, Global, WeakEntity, Window};
use rbx_cloud::PublishMode;

use crate::shell::import3d::ImportKind;
use crate::shell::{Export, Panel, PresetView, Shell};

use super::*;

/// The editor the menu commands act on. Weak: the listeners are global and
/// outlive the window, and a strong handle would keep a closed place alive.
struct MenuTarget(WeakEntity<Shell>);

impl Global for MenuTarget {}

/// The handle every listener below holds: resolved on each command, so one
/// set of listeners serves every editor.
#[derive(Clone, Copy)]
struct Target;

impl Target {
    fn update<R>(
        self,
        cx: &mut App,
        f: impl FnOnce(&mut Shell, &mut Context<Shell>) -> R,
    ) -> anyhow::Result<R> {
        let shell = cx
            .try_global::<MenuTarget>()
            .map(|target| target.0.clone())
            .ok_or_else(|| anyhow::anyhow!("no editor"))?;
        shell.update(cx, f)
    }

    /// A game-wide command, on the game the targeted place belongs to.
    fn game(
        self,
        cx: &mut App,
        f: impl FnOnce(
                &mut crate::shell::game::GameWorkspace,
                &mut Window,
                &mut Context<crate::shell::game::GameWorkspace>,
            ) + 'static,
    ) {
        let Ok(found) = self.update(cx, |shell, cx| shell.game_of(cx)) else {
            return;
        };
        if let Some((game, handle)) = found {
            let _ = handle.update(cx, |_, window, cx| {
                game.update(cx, |game, cx| f(game, window, cx))
            });
        }
    }
}

/// Points the menu commands at `shell`.
pub(crate) fn retarget(shell: &Entity<Shell>, cx: &mut App) {
    cx.set_global(MenuTarget(shell.downgrade()));
}

pub(super) fn install(shell: &Entity<Shell>, cx: &mut App) {
    let first = !cx.has_global::<MenuTarget>();
    retarget(shell, cx);
    if !first {
        return;
    }
    let shell = Target;
    cx.on_action(|_: &MenuOpenAutoSaves, cx| crate::recovery::open_folder(cx));
    cx.on_action({
        move |action: &MenuInsertTemplate, cx| {
            let index = action.index;
            let _ = shell.update(cx, |shell, cx| shell.insert_user_template(index, cx));
        }
    });
    cx.on_action({
        move |_: &MenuNewPlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.request_new(cx));
        }
    });
    cx.on_action({
        move |_: &MenuOpenPlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_place_dialog(cx));
        }
    });
    cx.on_action(move |_: &MenuOpenGame, cx| {
        let _ = shell.update(cx, |shell, cx| shell.open_game_dialog(cx));
    });
    cx.on_action(move |_: &MenuMarkShared, cx| {
        let _ = shell.update(cx, |shell, cx| shell.mark_shared(cx));
    });
    cx.on_action(move |_: &MenuUnmarkShared, cx| {
        let _ = shell.update(cx, |shell, cx| shell.unmark_shared(cx));
    });
    cx.on_action(move |_: &MenuFindInGame, cx| {
        shell.game(cx, |game, window, cx| game.open_find(window, cx));
    });
    cx.on_action(move |_: &MenuSyncShared, cx| {
        shell.game(cx, |game, window, cx| game.open_sync(window, cx));
    });
    cx.on_action(move |_: &MenuSaveAll, cx| {
        shell.game(cx, |game, _, cx| game.save_all(cx));
    });
    cx.on_action(move |_: &MenuPublishAll, cx| {
        shell.game(cx, |game, _, cx| game.ask_publish_all(cx));
    });
    cx.on_action({
        move |_: &MenuInsertObject, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_object_from_menu(cx));
        }
    });
    cx.on_action({
        move |_: &MenuToggleCommandBar, cx| {
            let _ = shell.update(cx, |shell, cx| shell.toggle_command_bar(cx));
        }
    });
    cx.on_action({
        move |_: &MenuSave, cx| {
            let _ = shell.update(cx, |shell, cx| shell.save(cx));
        }
    });
    cx.on_action({
        move |_: &MenuClosePlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.close_place(cx));
        }
    });
    cx.on_action({
        move |_: &MenuSaveToFile, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::Place, cx));
        }
    });
    cx.on_action({
        move |_: &MenuExportGltf, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::Gltf, cx));
        }
    });
    cx.on_action({
        move |_: &MenuExportProject, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::Project, cx));
        }
    });
    cx.on_action({
        move |_: &MenuExportClean, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::CleanPlace, cx));
        }
    });
    cx.on_action({
        move |_: &MenuSaveToRoblox, cx| {
            let _ = shell.update(cx, |shell, cx| {
                shell.upload_to_roblox(PublishMode::Saved, cx)
            });
        }
    });
    cx.on_action({
        move |_: &MenuPublishToRoblox, cx| {
            let _ = shell.update(cx, |shell, cx| {
                shell.upload_to_roblox(PublishMode::Published, cx)
            });
        }
    });
    cx.on_action({
        move |_: &MenuLinkRobloxPlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_roblox_link(None, cx));
        }
    });
    cx.on_action({
        move |_: &MenuVersionHistory, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_version_history(cx));
        }
    });
    cx.on_action({
        move |_: &MenuMergePlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_merge_place(cx));
        }
    });
    cx.on_action({
        move |_: &MenuGameSettings, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_game_settings(cx));
        }
    });
    cx.on_action({
        move |_: &MenuDataStores, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_data_stores(cx));
        }
    });
    cx.on_action({
        move |_: &MenuUpdatePlanning, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_update_planning(cx));
        }
    });
    cx.on_action({
        move |_: &MenuStudioSettings, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_settings(cx));
        }
    });
    cx.on_action({
        move |_: &MenuCustomizeShortcuts, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_shortcuts(cx));
        }
    });
    cx.on_action({
        move |_: &MenuBetaFeatures, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_settings_on("beta", cx));
        }
    });
    cx.on_action({
        move |_: &MenuRequireGraph, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_require_graph(cx));
        }
    });
    cx.on_action({
        move |_: &MenuScriptTemplates, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_script_templates(cx));
        }
    });
    cx.on_action({
        move |_: &MenuReduceMotion, cx| {
            let _ = shell.update(cx, |shell, cx| shell.toggle_reduce_motion(cx));
        }
    });
    cx.on_action({
        move |_: &MenuLargeTargets, cx| {
            let _ = shell.update(cx, |shell, cx| shell.toggle_large_targets(cx));
        }
    });
    cx.on_action({
        move |_: &MenuTopView, cx| {
            let _ = shell.update(cx, |shell, cx| shell.preset_view(PresetView::Top, cx));
        }
    });
    cx.on_action({
        move |_: &MenuFrontView, cx| {
            let _ = shell.update(cx, |shell, cx| shell.preset_view(PresetView::Front, cx));
        }
    });
    cx.on_action({
        move |_: &MenuRightView, cx| {
            let _ = shell.update(cx, |shell, cx| shell.preset_view(PresetView::Right, cx));
        }
    });
    cx.on_action({
        move |_: &MenuResetLayout, cx| {
            let _ = shell.update(cx, |shell, cx| shell.reset_layout(cx));
        }
    });
    // A dock closed from its own tab has no tab left to reopen it with, so
    // these are the way back, the same toggles the ribbon's Home tab
    // carries, going through the same call.
    for (panel, toggle) in [
        (Panel::Explorer, &MenuToggleExplorer as &dyn PanelToggle),
        (Panel::Properties, &MenuToggleProperties),
        (Panel::Output, &MenuToggleOutput),
        (Panel::Viewport, &MenuToggleViewport),
        (Panel::Collaborators, &MenuToggleCollaborators),
        (Panel::InputActions, &MenuToggleInputActions),
        (Panel::SecurityAudit, &MenuToggleSecurityAudit),
        (Panel::Playtests, &MenuTogglePlaytests),
        (Panel::Layers, &MenuToggleLayers),
        (Panel::Loren, &MenuToggleLoren),
        (Panel::AvatarSettings, &MenuToggleAvatarSettings),
    ] {
        toggle.install(panel, shell, cx);
    }
    cx.on_action({
        move |_: &MenuUndo, cx| {
            let _ = shell.update(cx, |shell, cx| shell.undo(cx));
        }
    });
    cx.on_action({
        move |_: &MenuRedo, cx| {
            let _ = shell.update(cx, |shell, cx| shell.redo(cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertPart, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Part", cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertFolder, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Folder", cx));
        }
    });
    cx.on_action({
        move |_: &MenuImportModel, cx| {
            let _ = shell.update(cx, |shell, cx| shell.choose_import(ImportKind::Model, cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertMesh, cx| {
            let _ = shell.update(cx, |shell, cx| shell.choose_import(ImportKind::Mesh, cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertRig, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_rig_dialog(cx));
        }
    });
    cx.on_action({
        move |_: &MenuImportImage, cx| {
            let _ = shell.update(cx, |shell, cx| shell.choose_import(ImportKind::Image, cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Script", cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertLocalScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("LocalScript", cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertModuleScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("ModuleScript", cx));
        }
    });
    cx.on_action({
        move |_: &MenuInsertModuleScriptClass, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_class_module(cx));
        }
    });
    cx.on_action({
        move |_: &MenuDeleteInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.delete_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuPasteAtOriginal, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_at_original(cx));
        }
    });
    cx.on_action({
        move |_: &MenuPasteIntoAtOriginal, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_into_at_original(cx));
        }
    });
    cx.on_action({
        move |_: &MenuCutInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.cut_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuCopyInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.copy_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuPasteInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_clipboard(cx));
        }
    });
    cx.on_action({
        move |_: &MenuPasteIntoInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_into_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuDuplicateInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.duplicate_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuGroup, cx| {
            let _ = shell.update(cx, |shell, cx| shell.group_selected(cx));
        }
    });
    cx.on_action({
        move |_: &MenuUngroup, cx| {
            let _ = shell.update(cx, |shell, cx| shell.ungroup_selected(cx));
        }
    });
    // Straight to the shell like every other item: raising the document
    // needs no `Window`. Going through `App::active_window` instead did
    // nothing at all wherever no window manager marks the window active
    // (a bare X server), since that is `None` there.
    cx.on_action({
        move |_: &MenuStyleEditor, cx| {
            let _ = shell.update(cx, |shell, cx| shell.reveal_style_editor(cx));
        }
    });
    cx.on_action({
        move |_: &MenuCommandPalette, cx| {
            let _ = shell.update(cx, |shell, cx| shell.request_palette(true, cx));
        }
    });
    cx.on_action({
        move |_: &MenuQuickOpen, cx| {
            let _ = shell.update(cx, |shell, cx| shell.request_palette(false, cx));
        }
    });
    cx.on_action(move |_: &MenuPlaceholder, _cx| {});
}

/// One View-menu entry per dock, registered the same way every other
/// action here is.
///
/// A trait only because `on_action` is generic over the action type and
/// three near-identical blocks read worse than one.
trait PanelToggle {
    fn install(&self, panel: Panel, shell: Target, cx: &mut App);
}

macro_rules! panel_toggle {
    ($action:ty) => {
        impl PanelToggle for $action {
            fn install(&self, panel: Panel, shell: Target, cx: &mut App) {
                cx.on_action(move |_: &$action, cx| {
                    let _ = shell.update(cx, |shell, cx| {
                        let showing = shell.is_panel_showing(panel);
                        shell.set_panel_open(panel, !showing, cx);
                    });
                });
            }
        }
    };
}

panel_toggle!(MenuToggleExplorer);
panel_toggle!(MenuToggleProperties);
panel_toggle!(MenuToggleOutput);
panel_toggle!(MenuToggleViewport);
panel_toggle!(MenuToggleCollaborators);
panel_toggle!(MenuToggleInputActions);
panel_toggle!(MenuToggleSecurityAudit);
panel_toggle!(MenuTogglePlaytests);
panel_toggle!(MenuToggleLayers);
panel_toggle!(MenuToggleLoren);
panel_toggle!(MenuToggleAvatarSettings);
