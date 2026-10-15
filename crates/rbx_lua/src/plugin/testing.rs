//! A minimal plugin VM for unit tests of single modules (see
//! `vm::build` for the real one).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::WeakDom;

use super::env::Env;

/// A VM over `dom` with the command bar's globals, `Env` app data (plugin
/// 1, named "Test", its `CoreGui`/`PluginGuiService` folders created as
/// roots of `dom`), `scheduler::install`, and — when `hooks` — the
/// `events::PluginHooks` hook slot. Not sandboxed. Returns the VM, its
/// `Env`, and the DOM slot (borrow it to inspect what scripts did).
pub(crate) fn test_vm(dom: WeakDom, hooks: bool) -> (Lua, Rc<Env>, Rc<RefCell<WeakDom>>) {
    todo!()
}
