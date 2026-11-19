//! Extension points for a host that adds members to the core bindings.
//!
//! Each VM installs one (as mlua app data). The command bar's gives
//! instances their events, attributes and bindables, plus `HttpService`'s
//! JSON and `CollectionService`; the plugin host's adds class-specific
//! plugin members, and `game:GetService` the services that have no
//! instance in a place file (`Selection`, `HttpService`, …).

use std::rc::Rc;

use mlua::{Lua, Result, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;

pub(crate) trait Hooks {
    /// Asked first by an instance's `__index` (after `Name`, `ClassName`
    /// and `Parent`): `Some` answers the read, `None` falls through to
    /// properties and children.
    fn index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Result<Option<Value>>;

    /// Asked first by an instance's `__newindex` (after `Name`, `ClassName`
    /// and `Parent`): `true` means handled, `false` falls through to the
    /// property write.
    fn new_index(
        &self,
        lua: &Lua,
        ctx: &Ctx,
        referent: Ref,
        class: &str,
        key: &str,
        value: &Value,
    ) -> Result<bool>;

    /// Asked first by `game:GetService(name)`.
    fn service(&self, lua: &Lua, ctx: &Ctx, name: &str) -> Result<Option<Value>>;

    /// Asked by `game`'s `__index` for a member it does not have itself
    /// (its signals, in the plugin host).
    fn game_member(&self, _lua: &Lua, _key: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    /// Told after a script's `Name`, `Parent` or property write went
    /// through (a running server fires that write's events right away).
    fn after_write(&self, _lua: &Lua) -> Result<()> {
        Ok(())
    }
}

/// [`Hooks::after_write`] of the VM's hooks, if it has any.
pub(crate) fn wrote(lua: &Lua) -> Result<()> {
    match hooks(lua) {
        Some(hooks) => hooks.after_write(lua),
        None => Ok(()),
    }
}

/// The app-data slot a VM's hooks live in.
pub(crate) struct HookSlot(pub(crate) Rc<dyn Hooks>);

pub(crate) fn hooks(lua: &Lua) -> Option<Rc<dyn Hooks>> {
    lua.app_data_ref::<HookSlot>().map(|slot| slot.0.clone())
}
