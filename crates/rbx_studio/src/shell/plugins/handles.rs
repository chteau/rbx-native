//! The plugins' `Handles`, `ArcHandles` and `*HandleAdornment`s in the 3D
//! view: drawn through the render thread's mirror (`pump::Command::
//! PluginAdornments`), picked and dragged by `workspace_view::
//! plugin_handles`, which reports back as `ViewportAction::PluginHandle`.

use gpui_kit::*;
use rbx_lua::plugin::{HandleEvent, Input};

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
        let adornments = self
            .plugins
            .host
            .as_ref()
            .map(|host| host.adornments())
            .unwrap_or_default();
        if adornments.is_empty() && self.plugins.handles.sent.is_empty() {
            return;
        }
        let shapes = rbx_viewer::handle_pick::shapes(&self.dom, &self.database, &adornments);
        self.plugins.handles.sent = adornments.iter().map(|a| a.referent()).collect();
        self.viewport
            .update(cx, |view, _| view.set_plugin_handles(adornments, shapes));
    }

    /// `ViewportAction::PluginHandle`.
    pub(in crate::shell) fn plugin_handle_event(
        &mut self,
        event: HandleEvent,
        cx: &mut Context<Self>,
    ) {
        self.plugin_input(Input::Handle(event), cx);
    }
}
