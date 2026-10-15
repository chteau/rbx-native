//! Renders plugin widgets off the editor thread with a second
//! `rbx_viewer::Headless` built from each widget's own small DOM
//! (`PluginHost::widget_dom`), created on first use.

use rbx_dom::WeakDom;
use rbx_lua::plugin::UiId;
use rbx_viewer::GuiBox;

/// One finished render.
#[derive(Clone)]
pub(crate) struct Drawn {
    pub(crate) revision: u64,
    pub(crate) size: (u32, u32),
    /// RGBA8, straight alpha, as a GPUI image.
    pub(crate) image: std::sync::Arc<gpui_kit::RenderImage>,
    /// Paint order; `boxes[0]` is the widget's own frame.
    pub(crate) boxes: Vec<GuiBox>,
}

/// The render thread's handle.
pub(crate) struct Renderer {
    // channels — implementer's choice
}

impl Renderer {
    pub(crate) fn new() -> Self {
        todo!()
    }

    /// Queues a render (`backdrop` = the panel's background, encoded sRGB);
    /// a newer request for the same widget replaces a queued one.
    pub(crate) fn request(&self, widget: UiId, revision: u64, dom: WeakDom, size: (u32, u32), backdrop: [f32; 3]) {
        todo!()
    }

    /// Renders finished since the last call.
    pub(crate) fn finished(&self) -> Vec<(UiId, Drawn)> {
        todo!()
    }
}
