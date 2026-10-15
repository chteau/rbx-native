//! The Manage Plugins dialog: every plugin found, where it came from,
//! whether it runs, its last error; a switch per plugin (persisted), the
//! "Load Roblox Studio's plugins" switch, Open Folder and Reload.

use gpui_kit::*;

use crate::shell::Shell;

/// What the user chose in the dialog, kept in `<config dir>/plugins.json`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Prefs {
    /// Paths of plugins switched off.
    pub(crate) disabled: Vec<String>,
    /// Also load Roblox Studio's own Plugins folder (on by default).
    pub(crate) load_studio: bool,
}

/// The saved prefs, or the defaults when there are none (or they do not
/// parse).
pub(crate) fn load_prefs() -> Prefs {
    todo!()
}

/// Saves atomically (`settings::write_atomic`); failures are ignored, as
/// a lost preference costs a click.
pub(crate) fn save_prefs(prefs: &Prefs) {
    todo!()
}

impl Shell {
    pub(super) fn open_plugin_manager(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    pub(super) fn plugin_manager_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        todo!()
    }
}
