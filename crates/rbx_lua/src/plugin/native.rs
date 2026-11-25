//! The `rbxnative` global, for plugins with a `plugin.toml`.

use mlua::{Function, Lua, Result};

use rbx_permissions::{manifest_text, parse_manifest, Capability, Decision};

use super::api::{Permission, Request};
use super::env::env;
use super::plugin_object;
use crate::engine::signal::{Owner, SignalKey};

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
        perms.set(i + 1, manifest_text(p).unwrap_or_default())?;
    }
    info.set("permissions", perms)?;
    table.set("manifest", info)?;

    table.set(
        "hasPermission",
        lua.create_function(|lua, text: String| {
            Ok(match parse_manifest(&text) {
                Some(Permission::Capability(capability)) => allowed(lua, capability),
                Some(Permission::Host(host)) => host_decision(lua, &host) == Decision::Allow,
                None => false,
            })
        })?,
    )?;

    table.set(
        "notify",
        lua.create_function(|lua, text: String| {
            let env = env(lua);
            let plugin = env.plugin;
            env.shared
                .borrow_mut()
                .requests
                .push(Request::Notify { plugin, text });
            Ok(())
        })?,
    )?;

    table.set(
        "command",
        lua.create_function(
            |lua,
             (id, text, callback, tooltip, icon): (
                String,
                String,
                Function,
                Option<String>,
                Option<String>,
            )| {
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
                // Not stamped: no DOM change can be fired at a button.
                env.engine
                    .signals
                    .borrow_mut()
                    .connect(&key, callback, false, None);
                Ok(())
            },
        )?,
    )?;

    lua.globals().set("rbxnative", table)
}

/// Whether the running plugin may use `capability`, as the permission
/// store says right now.
pub(crate) fn allowed(lua: &Lua, capability: Capability) -> bool {
    let env = env(lua);
    let store = env.shared.borrow().permissions.clone();
    let allowed = store.borrow().policy().allows(&env.profile, capability);
    allowed
}

/// What the store says about HTTP from the running plugin to `host`.
pub(crate) fn host_decision(lua: &Lua, host: &str) -> Decision {
    let env = env(lua);
    let store = env.shared.borrow().permissions.clone();
    let decision = store.borrow().policy().host(&env.profile, host);
    decision
}
