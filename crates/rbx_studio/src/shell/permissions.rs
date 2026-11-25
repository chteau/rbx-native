//! The shell's end of `rbx_permissions`: the one store MCP clients and
//! plugins are checked against, the plugins Settings › Permissions lists,
//! and the one way that page changes what they may do.

use gpui_kit::Context;
use rbx_permissions::{Policy, PluginProfile, Store};

use super::Shell;

/// `<config>/permissions.json`, or memory only without a config folder.
pub(super) fn open() -> rbx_permissions::Shared {
    match crate::settings::default_config_dir() {
        Some(dir) => Store::open(dir.join("permissions.json")),
        None => Store::default(),
    }
    .shared()
}

impl Shell {
    pub(crate) fn permissions(&self) -> rbx_permissions::Shared {
        self.permissions.clone()
    }

    /// Every loaded plugin, Luau and WASM, as the store knows them, in the
    /// order the plugin manager lists them.
    pub(crate) fn plugin_profiles(&self) -> Vec<PluginProfile> {
        let luau = self.plugins.host.iter().flat_map(|host| {
            host.plugins()
                .into_iter()
                .map(|info| info.origin.profile(&info.name))
        });
        let wasm = self.plugins.wasm.iter().map(|entry| entry.profile.clone());
        let mut profiles: Vec<PluginProfile> = luau.chain(wasm).collect();
        profiles.sort_by_key(|p| p.name.to_lowercase());
        profiles.dedup_by(|a, b| a.name == b.name);
        profiles
    }

    /// Applies one change from Settings › Permissions; it reaches the very
    /// next check, and is saved.
    pub(crate) fn change_permissions(
        &mut self,
        change: impl FnOnce(&mut Policy),
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.permissions.borrow_mut().change(change) {
            self.output
                .push_warning(&format!("Permissions could not be saved: {error}"));
        }
        cx.notify();
    }
}
