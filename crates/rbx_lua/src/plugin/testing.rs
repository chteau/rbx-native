//! A minimal plugin VM for unit tests of single modules (see
//! `vm::build` for the real one).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::env::{engine_for, Env, Shared};
use super::events::PluginHooks;
use crate::ctx::Ctx;
use crate::hooks::HookSlot;

/// A VM over `dom` with the command bar's globals, `Env` app data (plugin
/// 1, named "Test", its `CoreGui`/`PluginGuiService` folders created as
/// roots of `dom`), `engine::install`, and — when `hooks` — the
/// `events::PluginHooks` hook slot. Not sandboxed. Returns the VM, its
/// `Env`, and the DOM slot (borrow it to inspect what scripts did).
pub(crate) fn test_vm(mut dom: WeakDom, hooks: bool) -> (Lua, Rc<Env>, Rc<RefCell<WeakDom>>) {
    let core_gui = dom.new_instance("Folder", "CoreGui", None);
    let plugin_gui_service = dom.new_instance("Folder", "PluginGuiService", None);
    let slot = Rc::new(RefCell::new(dom));
    let ctx = Ctx::new(
        slot.clone(),
        Rc::new(ReflectionDatabase::embedded()),
        Rc::default(),
    );
    let shared = Rc::new(RefCell::new(Shared::default()));
    let roots = Rc::new(RefCell::new(vec![core_gui, plugin_gui_service]));
    let env = Rc::new(Env {
        plugin: 1,
        name: "Test".to_string(),
        engine: engine_for("Test", &ctx, &shared, &roots),
        ctx: ctx.clone(),
        shared,
        manifest: None,
        roots,
        core_gui,
        plugin_gui_service,
    });
    let lua = Lua::new();
    if hooks {
        lua.set_app_data(HookSlot(Rc::new(PluginHooks)));
    }
    lua.set_app_data(env.clone());
    crate::runtime::install_globals(&lua, &ctx).expect("install globals");
    crate::engine::install(&lua, env.engine.clone()).expect("install engine");
    (lua, env, slot)
}
