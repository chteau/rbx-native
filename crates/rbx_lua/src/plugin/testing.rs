//! A minimal plugin VM for unit tests of single modules (see
//! `vm::build` for the real one).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::env::{Env, Shared};
use super::events::PluginHooks;
use super::scheduler::{self, Scheduler};
use super::signal::Signals;
use crate::ctx::Ctx;
use crate::hooks::HookSlot;

/// A VM over `dom` with the command bar's globals, `Env` app data (plugin
/// 1, named "Test", its `CoreGui`/`PluginGuiService` folders created as
/// roots of `dom`), `scheduler::install`, and — when `hooks` — the
/// `events::PluginHooks` hook slot. Not sandboxed. Returns the VM, its
/// `Env`, and the DOM slot (borrow it to inspect what scripts did).
pub(crate) fn test_vm(mut dom: WeakDom, hooks: bool) -> (Lua, Rc<Env>, Rc<RefCell<WeakDom>>) {
    let core_gui = dom.new_instance("Folder", "CoreGui", None);
    let plugin_gui_service = dom.new_instance("Folder", "PluginGuiService", None);
    let slot = Rc::new(RefCell::new(dom));
    let ctx = Ctx::new(
        slot.clone(),
        Rc::new(ReflectionDatabase::embedded()),
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(RefCell::new(Vec::new())),
    );
    let env = Rc::new(Env {
        plugin: 1,
        name: "Test".to_string(),
        ctx: ctx.clone(),
        shared: Rc::new(RefCell::new(Shared::default())),
        signals: RefCell::new(Signals::default()),
        scheduler: RefCell::new(Scheduler::default()),
        manifest: None,
        roots: RefCell::new(vec![core_gui, plugin_gui_service]),
        modules: RefCell::new(HashMap::new()),
        core_gui,
        plugin_gui_service,
    });
    let lua = Lua::new();
    if hooks {
        lua.set_app_data(HookSlot(Rc::new(PluginHooks)));
    }
    lua.set_app_data(env.clone());
    crate::runtime::install_globals(&lua, &ctx).expect("install globals");
    scheduler::install(&lua).expect("install scheduler");
    (lua, env, slot)
}
