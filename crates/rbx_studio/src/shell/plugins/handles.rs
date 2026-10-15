//! The plugins' `Handles`, `ArcHandles` and `*HandleAdornment`s in the 3D
//! view: drawn through the render thread's mirror (`pump::Command::
//! PluginAdornments`), picked and dragged by `workspace_view::
//! plugin_handles`, which reports back as `ViewportAction::PluginHandle`.

use gpui_kit::*;
use rbx_lua::plugin::HandleEvent;

use crate::shell::Shell;

#[derive(Default)]
pub(crate) struct Handles {
    /// The adornments last sent to the view.
    pub(crate) sent: Vec<rbx_dom::Ref>,
}

impl Shell {
    /// After `adornments_changed` (or a change to an adornee): sends
    /// `PluginHost::adornments()` to the render mirror and their pickable
    /// shapes to the view.
    pub(super) fn sync_plugin_handles(&mut self, cx: &mut Context<Self>) {
        todo!()
    }

    /// `ViewportAction::PluginHandle`.
    pub(in crate::shell) fn plugin_handle_event(&mut self, event: HandleEvent, cx: &mut Context<Self>) {
        todo!()
    }
}
