//! Plugin widgets: the Plugin Widgets dock panel (one tab per enabled
//! `DockWidgetPluginGui`) and floating widgets drawn above the editor.
//! Each is rendered by `render` and hit-tested on its boxes: pointer and
//! key input become `Input::Gui`, a `TextBox` is edited through an input
//! field placed over it.

use std::collections::HashMap;

use gpui_kit::*;
use rbx_lua::plugin::UiId;

use super::render;
use crate::shell::Shell;

#[derive(Default)]
pub(crate) struct Widgets {
    /// Last render per widget.
    pub(crate) drawn: HashMap<UiId, render::Drawn>,
    /// The dock tab shown.
    pub(crate) current: Option<UiId>,
    pub(crate) renderer: Option<render::Renderer>,
}

impl Shell {
    /// The Plugin Widgets panel's content (for `workspace::panel_parts`).
    pub(in crate::shell) fn plugin_widgets_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        todo!()
    }

    /// Floating widgets, for `plugin_overlays`.
    pub(super) fn floating_widget_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        todo!()
    }

    /// After `ui_changed`: opens the panel when a dock widget became
    /// enabled, re-requests renders whose widget revision changed.
    pub(super) fn plugin_widgets_changed(&mut self, cx: &mut Context<Self>) {
        todo!()
    }
}
