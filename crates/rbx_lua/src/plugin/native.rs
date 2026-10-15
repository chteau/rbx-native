//! The `rbxnative` global, for plugins with a `plugin.toml`.

use mlua::{Lua, Result};

use super::api::Permission;

/// Installs `rbxnative` when the VM's `Env::manifest` is set:
/// `version` (this crate's), `manifest` (`{name, version, description,
/// permissions = {strings}}`), `hasPermission(text)`, `notify(text)` →
/// `Request::Notify`, `command(id, text, callback, tooltip?, icon?)` → a
/// button on a toolbar named after the plugin whose `Click` runs
/// `callback`. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    todo!()
}

/// `dom.read`, `dom.write`, `selection`, `debugger`, `http:<host>`.
pub(crate) fn parse_permission(text: &str) -> Option<Permission> {
    todo!()
}

/// The inverse of [`parse_permission`].
pub(crate) fn permission_text(permission: &Permission) -> String {
    todo!()
}

/// Whether the running plugin may use `permission`: always for a Roblox
/// plugin; for a native one, when its manifest lists it (`Http("*")`
/// covering every host).
pub(crate) fn allowed(lua: &Lua, permission: &Permission) -> bool {
    todo!()
}
