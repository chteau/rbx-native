//! File › Customize Shortcuts: real Studio's own window for viewing and
//! rebinding every hotkey, a window of its own rather than a Settings page,
//! as Studio keeps it. Rows come from `crate::shortcuts::ALL`; every change
//! goes through `shortcuts::replace`, so it applies at once everywhere.

use gpui_kit::*;

use super::Shell;

/// Opens the window with the editor, for a capture.
pub(crate) const OPEN_VARIABLE: &str = "RBX_STUDIO_SHORTCUTS";

impl Shell {
    /// Brings Customize Shortcuts forward, opening it if it isn't.
    pub(crate) fn open_shortcuts(&mut self, cx: &mut Context<Self>) {
        todo!()
    }
}
