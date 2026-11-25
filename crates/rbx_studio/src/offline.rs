//! What greys out while offline. Every command that needs Roblox's servers
//! (an upload, a cloud setting, a download) is disabled with [`TIP`] while
//! `rbx_net` says the app is offline, and live again as soon as it isn't:
//! the windows re-render when the status changes (`shell::credential_watch`,
//! `launcher::home_window`), and every check here reads it fresh.

use gpui_kit::Action;

use crate::menu_bar::{
    MenuDataStores, MenuGameSettings, MenuImportImage, MenuImportModel, MenuImportVideo,
    MenuInsertMesh, MenuLinkRobloxPlace, MenuPublishAll, MenuPublishToRoblox, MenuSaveToRoblox,
    MenuVersionHistory,
};

/// The tooltip on a command greyed out offline.
pub(crate) const TIP: &str = "You\u{2019}re offline";

/// Whether commands that need Roblox are unavailable right now.
pub(crate) fn blocked() -> bool {
    rbx_net::is_offline()
}

/// Whether a menu-bar command needs Roblox's servers to do anything at all.
/// The imports are uploads: a mesh or an image is only usable in a place as
/// a Roblox asset (`shell::import3d`).
pub(crate) fn needs_roblox(action: &dyn Action) -> bool {
    let any = action.as_any();
    any.is::<MenuSaveToRoblox>()
        || any.is::<MenuPublishToRoblox>()
        || any.is::<MenuPublishAll>()
        || any.is::<MenuLinkRobloxPlace>()
        || any.is::<MenuVersionHistory>()
        || any.is::<MenuGameSettings>()
        || any.is::<MenuDataStores>()
        || any.is::<MenuImportModel>()
        || any.is::<MenuInsertMesh>()
        || any.is::<MenuImportImage>()
        || any.is::<MenuImportVideo>()
}

/// The MCP tools that only call Roblox: refused at once offline, with
/// [`mcp_refusal`], rather than failing on the first request.
pub(crate) const MCP_TOOLS: [&str; 5] = [
    "search_asset",
    "insert_asset",
    "import_model",
    "upload_image",
    "http_get",
];

/// What an MCP tool refused offline answers.
pub(crate) fn mcp_refusal(tool: &str) -> String {
    format!(
        "{TIP}: {tool} needs Roblox\u{2019}s servers. {}.",
        rbx_net::status().reason()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_bar::{MenuInsertRig, MenuSave, MenuSaveToFile};

    #[test]
    fn only_the_cloud_commands_need_roblox() {
        for action in [
            &MenuSaveToRoblox as &dyn Action,
            &MenuPublishToRoblox,
            &MenuPublishAll,
            &MenuLinkRobloxPlace,
            &MenuVersionHistory,
            &MenuGameSettings,
            &MenuDataStores,
            &MenuImportModel,
            &MenuInsertMesh,
            &MenuImportImage,
            &MenuImportVideo,
        ] {
            assert!(needs_roblox(action), "{}", action.name());
        }
        assert!(!needs_roblox(&MenuSave));
        assert!(!needs_roblox(&MenuSaveToFile));
        assert!(!needs_roblox(&MenuInsertRig));
    }
}
