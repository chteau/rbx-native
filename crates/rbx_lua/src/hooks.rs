//! Extension points for a host that adds members to the core bindings.
//!
//! The command bar's `Runtime` installs none. The plugin host installs one
//! per VM (as mlua app data) to give instances their events and
//! class-specific plugin members, and `game:GetService` the services that
//! have no instance in a place file (`Selection`, `HttpService`, …).

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
}

/// The app-data slot a VM's hooks live in.
pub(crate) struct HookSlot(pub(crate) Rc<dyn Hooks>);

pub(crate) fn hooks(lua: &Lua) -> Option<Rc<dyn Hooks>> {
    lua.app_data_ref::<HookSlot>().map(|slot| slot.0.clone())
}
