//! The Manage Plugins dialog: every plugin found, where it came from,
//! whether it runs, its last error; a switch per plugin (persisted), the
//! "Load Roblox Studio's plugins" switch, Open Folder and Reload.

use gpui_kit::*;

use crate::shell::Shell;

impl Shell {
    pub(super) fn open_plugin_manager(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    pub(super) fn plugin_manager_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        todo!()
    }
}
