//! The Plugins ribbon tab: one group per plugin toolbar (and per WASM
//! plugin's commands), then Manage / Folder / Reload.

use gpui_kit::*;

use crate::shell::Shell;

impl Shell {
    /// The tab's groups, as `ribbon` lays them out. A button is a tile with
    /// its icon (resolved by `icon_path`, a generic plug icon otherwise),
    /// its text, its tooltip, the selected look while `active`, disabled
    /// while not `enabled`; clicking it sends `Input::ButtonClicked`.
    pub(in crate::shell) fn plugin_ribbon_groups(&self, cx: &mut Context<Self>) -> Vec<Vec<AnyElement>> {
        todo!()
    }
}

/// A local image for a plugin icon: an existing file path as is,
/// `rbxasset://textures/...` under Studio's content folder when found,
/// `rbxassetid://<id>` from the asset cache once downloaded (requesting it
/// otherwise); `None` while unavailable.
pub(crate) fn icon_path(icon: &str) -> Option<std::path::PathBuf> {
    todo!()
}
