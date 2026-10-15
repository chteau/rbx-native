//! The `rbxnative` global, for plugins with a `plugin.toml`.

use mlua::{Function, Lua, Result};

use super::api::{Permission, Request};
use super::env::env;
use super::plugin_object;
use super::signal::{Connection, Owner, SignalKey};

/// Installs `rbxnative` when the VM's `Env::manifest` is set:
/// `version` (this crate's), `manifest` (`{name, version, description,
/// permissions = {strings}}`), `hasPermission(text)`, `notify(text)` →
/// `Request::Notify`, `command(id, text, callback, tooltip?, icon?)` → a
/// button on a toolbar named after the plugin whose `Click` runs
/// `callback`. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let Some(manifest) = env(lua).manifest.clone() else {
        return Ok(());
    };
    let table = lua.create_table()?;
    table.set("version", env!("CARGO_PKG_VERSION"))?;

    let info = lua.create_table()?;
    info.set("name", manifest.name.as_str())?;
    info.set("version", manifest.version.as_str())?;
    info.set("description", manifest.description.as_str())?;
    let perms = lua.create_table()?;
    for (i, p) in manifest.permissions.iter().enumerate() {
        perms.set(i + 1, permission_text(p))?;
    }
    info.set("permissions", perms)?;
    table.set("manifest", info)?;

    table.set(
        "hasPermission",
        lua.create_function(|lua, text: String| {
            Ok(parse_permission(&text).is_some_and(|p| allowed(lua, &p)))
        })?,
    )?;

    table.set(
        "notify",
        lua.create_function(|lua, text: String| {
            let env = env(lua);
            let plugin = env.plugin;
            env.shared.borrow_mut().requests.push(Request::Notify { plugin, text });
            Ok(())
        })?,
    )?;

    table.set(
        "command",
        lua.create_function(
            |lua, (id, text, callback, tooltip, icon): (String, String, Function, Option<String>, Option<String>)| {
                let env = env(lua);
                let button = plugin_object::toolbar_button(
                    lua,
                    &env.name,
                    &id,
                    tooltip.as_deref().unwrap_or(&text),
                    icon.as_deref().unwrap_or(""),
                    &text,
                )?;
                let key = SignalKey::new(Owner::Ui(button), "Click");
                let mut signals = env.signals.borrow_mut();
                signals.next += 1;
                let conn_id = signals.next;
                signals.connections.entry(key).or_default().push(Connection {
                    id: conn_id,
                    func: callback,
                    once: false,
                });
                Ok(())
            },
        )?,
    )?;

    lua.globals().set("rbxnative", table)
}

/// `dom.read`, `dom.write`, `selection`, `debugger`, `http:<host>`.
pub(crate) fn parse_permission(text: &str) -> Option<Permission> {
    match text {
        "dom.read" => Some(Permission::DomRead),
        "dom.write" => Some(Permission::DomWrite),
        "selection" => Some(Permission::Selection),
        "debugger" => Some(Permission::Debugger),
        _ => text
            .strip_prefix("http:")
            .filter(|h| !h.is_empty())
            .map(|h| Permission::Http(h.to_string())),
    }
}

/// The inverse of [`parse_permission`].
pub(crate) fn permission_text(permission: &Permission) -> String {
    match permission {
        Permission::DomRead => "dom.read".to_string(),
        Permission::DomWrite => "dom.write".to_string(),
        Permission::Selection => "selection".to_string(),
        Permission::Debugger => "debugger".to_string(),
        Permission::Http(host) => format!("http:{host}"),
    }
}

/// Whether the running plugin may use `permission`: always for a Roblox
/// plugin; for a native one, when its manifest lists it (`Http("*")`
/// covering every host).
pub(crate) fn allowed(lua: &Lua, permission: &Permission) -> bool {
    let env = env(lua);
    let Some(manifest) = &env.manifest else {
        return true;
    };
    manifest.permissions.iter().any(|p| {
        p == permission || (matches!(permission, Permission::Http(_)) && *p == Permission::Http("*".to_string()))
    })
}
