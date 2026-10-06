//! One `App::on_action` per wired menu command, registered once from
//! `menu_bar::build`.
//!
//! Global rather than an element-tree `on_action`, because
//! `Context::on_action` may only be registered during an entity's own paint
//! pass — `build` runs once, from `Shell::new`, well before `Shell` ever
//! renders.

use gpui_kit::{App, Entity, WeakEntity};
use rbx_cloud::PublishMode;

use crate::shell::{Export, Panel, Shell};

use super::*;

pub(super) fn install(shell: Entity<Shell>, cx: &mut App) {
    // Weak: these listeners are global and outlive the window. File › Close
    // Place and a later open install a second set for the next editor, and
    // a strong handle here would keep every closed place's Shell alive.
    let shell = shell.downgrade();
    cx.on_action(|_: &MenuOpenAutoSaves, cx| crate::recovery::open_folder(cx));
    cx.on_action({
        let shell = shell.clone();
        move |action: &MenuInsertTemplate, cx| {
            let index = action.index;
            let _ = shell.update(cx, |shell, cx| shell.insert_user_template(index, cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuSave, cx| {
            let _ = shell.update(cx, |shell, cx| shell.save(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuClosePlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.close_place(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuSaveToFile, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::Place, cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuExportGltf, cx| {
            let _ = shell.update(cx, |shell, cx| shell.export_place(Export::Gltf, cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuSaveToRoblox, cx| {
            let _ = shell.update(cx, |shell, cx| {
                shell.upload_to_roblox(PublishMode::Saved, cx)
            });
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuPublishToRoblox, cx| {
            let _ = shell.update(cx, |shell, cx| {
                shell.upload_to_roblox(PublishMode::Published, cx)
            });
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuLinkRobloxPlace, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_roblox_link(None, cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuVersionHistory, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_version_history(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuStudioSettings, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_settings(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuScriptTemplates, cx| {
            let _ = shell.update(cx, |shell, cx| shell.open_script_templates(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuReduceMotion, cx| {
            let _ = shell.update(cx, |shell, cx| shell.toggle_reduce_motion(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuLargeTargets, cx| {
            let _ = shell.update(cx, |shell, cx| shell.toggle_large_targets(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuResetLayout, cx| {
            let _ = shell.update(cx, |shell, cx| shell.reset_layout(cx));
        }
    });
    // A dock closed from its own tab has no tab left to reopen it with, so
    // these are the way back — the same toggles the ribbon's Home tab
    // carries, going through the same call.
    for (panel, toggle) in [
        (Panel::Explorer, &MenuToggleExplorer as &dyn PanelToggle),
        (Panel::Properties, &MenuToggleProperties),
        (Panel::Output, &MenuToggleOutput),
        (Panel::Viewport, &MenuToggleViewport),
    ] {
        toggle.install(panel, shell.clone(), cx);
    }
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuUndo, cx| {
            let _ = shell.update(cx, |shell, cx| shell.undo(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuRedo, cx| {
            let _ = shell.update(cx, |shell, cx| shell.redo(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertPart, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Part", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertFolder, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Folder", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("Script", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertLocalScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("LocalScript", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertModuleScript, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_instance("ModuleScript", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertModuleScriptClass, cx| {
            let _ = shell.update(cx, |shell, cx| shell.insert_class_module(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuDeleteInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.delete_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuCutInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.cut_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuCopyInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.copy_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuPasteInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_clipboard(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuPasteIntoInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.paste_into_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuDuplicateInstance, cx| {
            let _ = shell.update(cx, |shell, cx| shell.duplicate_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuGroup, cx| {
            let _ = shell.update(cx, |shell, cx| shell.group_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuUngroup, cx| {
            let _ = shell.update(cx, |shell, cx| shell.ungroup_selected(cx));
        }
    });
    // Straight to the shell like every other item: raising the document
    // needs no `Window`. Going through `App::active_window` instead did
    // nothing at all wherever no window manager marks the window active
    // (a bare X server), since that is `None` there.
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuStyleEditor, cx| {
            let _ = shell.update(cx, |shell, cx| shell.reveal_style_editor(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuCommandPalette, cx| {
            let _ = shell.update(cx, |shell, cx| shell.request_palette(true, cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
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
    fn install(&self, panel: Panel, shell: WeakEntity<Shell>, cx: &mut App);
}

macro_rules! panel_toggle {
    ($action:ty) => {
        impl PanelToggle for $action {
            fn install(&self, panel: Panel, shell: WeakEntity<Shell>, cx: &mut App) {
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
