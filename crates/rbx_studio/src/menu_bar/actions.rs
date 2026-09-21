//! One `App::on_action` per wired menu command, registered once from
//! `menu_bar::build`.
//!
//! Global rather than an element-tree `on_action`, because
//! `Context::on_action` may only be registered during an entity's own paint
//! pass — `build` runs once, from `Shell::new`, well before `Shell` ever
//! renders.

use gpui_kit::{App, Entity};

use crate::shell::{Panel, Shell};

use super::*;

pub(super) fn install(shell: Entity<Shell>, cx: &mut App) {
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuSave, cx| {
            shell.update(cx, |shell, cx| shell.save(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuReduceMotion, cx| {
            shell.update(cx, |shell, cx| shell.toggle_reduce_motion(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuLargeTargets, cx| {
            shell.update(cx, |shell, cx| shell.toggle_large_targets(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuResetLayout, cx| {
            shell.update(cx, |shell, cx| shell.reset_layout(cx));
        }
    });
    // A dock closed from its own tab has no tab left to reopen it with, so
    // these are the way back — the same toggles the ribbon's Home tab
    // carries, going through the same call.
    for (panel, toggle) in [
        (Panel::Explorer, &MenuToggleExplorer as &dyn PanelToggle),
        (Panel::Properties, &MenuToggleProperties),
        (Panel::Output, &MenuToggleOutput),
    ] {
        toggle.install(panel, shell.clone(), cx);
    }
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
        move |_: &MenuInsertScript, cx| {
            shell.update(cx, |shell, cx| shell.insert_instance("Script", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertLocalScript, cx| {
            shell.update(cx, |shell, cx| shell.insert_instance("LocalScript", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertModuleScript, cx| {
            shell.update(cx, |shell, cx| shell.insert_instance("ModuleScript", cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuInsertModuleScriptClass, cx| {
            shell.update(cx, |shell, cx| shell.insert_class_module(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuDeleteInstance, cx| {
            shell.update(cx, |shell, cx| shell.delete_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuCutInstance, cx| {
            shell.update(cx, |shell, cx| shell.cut_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuCopyInstance, cx| {
            shell.update(cx, |shell, cx| shell.copy_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuPasteInstance, cx| {
            shell.update(cx, |shell, cx| shell.paste_clipboard(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuPasteIntoInstance, cx| {
            shell.update(cx, |shell, cx| shell.paste_into_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuDuplicateInstance, cx| {
            shell.update(cx, |shell, cx| shell.duplicate_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuGroup, cx| {
            shell.update(cx, |shell, cx| shell.group_selected(cx));
        }
    });
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuUngroup, cx| {
            shell.update(cx, |shell, cx| shell.ungroup_selected(cx));
        }
    });
    // The one item here that needs a `Window`: raising a dock tab moves a
    // panel, and `App::on_action` hands this handler only an `App`. The
    // active window is this app's only window.
    cx.on_action({
        let shell = shell.clone();
        move |_: &MenuStyleEditor, cx| {
            let Some(window) = cx.active_window() else {
                return;
            };
            let shell = shell.clone();
            let _ = window.update(cx, move |_, _window, cx| {
                shell.update(cx, |shell, cx| shell.reveal_style_editor(cx));
            });
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
    fn install(&self, panel: Panel, shell: Entity<Shell>, cx: &mut App);
}

macro_rules! panel_toggle {
    ($action:ty) => {
        impl PanelToggle for $action {
            fn install(&self, panel: Panel, shell: Entity<Shell>, cx: &mut App) {
                cx.on_action(move |_: &$action, cx| {
                    shell.update(cx, |shell, cx| {
                        let open = shell.is_panel_open(panel);
                        shell.set_panel_open(panel, !open, cx);
                    });
                });
            }
        }
    };
}

panel_toggle!(MenuToggleExplorer);
panel_toggle!(MenuToggleProperties);
panel_toggle!(MenuToggleOutput);
