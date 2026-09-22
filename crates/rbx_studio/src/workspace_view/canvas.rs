//! The UI thread's half of the UI editor's canvas: the request it forwards to
//! the render thread (see `pump::canvas`), and the last frame that came back.
//!
//! The canvas is drawn by this view's render thread because that is where
//! the viewer — and so the GUI renderer — lives; it is shown by `Shell`'s UI
//! editor, which reads the frame from here and repaints on [`CanvasUpdated`].

use std::sync::Arc;

use gpui_kit::{Context, RenderImage, Window};
use rbx_viewer::GuiBox;

use super::pump::canvas::{Drawn, Request};
use super::{frame, WorkspaceView};

/// A new canvas frame has arrived; whoever shows it repaints.
pub(crate) struct CanvasUpdated;

impl gpui_kit::EventEmitter<CanvasUpdated> for WorkspaceView {}

/// The last canvas drawn: what it was asked for, the picture, and every
/// element as laid out, in paint order.
pub(crate) struct Canvas {
    pub(crate) request: Request,
    /// The size drawn — see `pump::canvas::Drawn::size`.
    pub(crate) size: (u32, u32),
    pub(crate) image: Arc<RenderImage>,
    pub(crate) boxes: Vec<GuiBox>,
}

impl WorkspaceView {
    /// Asks for `request` to be drawn, or for no canvas at all. Called every
    /// time the canvas renders; only a change travels to the render thread.
    pub(crate) fn set_canvas(&mut self, request: Option<Request>) {
        if request != self.canvas_request {
            self.canvas_request = request;
            self.pump.canvas(request);
        }
    }

    pub(crate) fn canvas(&self) -> Option<&Canvas> {
        self.canvas.as_ref()
    }

    /// Takes a drawn canvas off the render thread and tells `Shell`.
    pub(super) fn show_canvas(
        &mut self,
        drawn: Drawn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (width, height) = drawn.size;
        let Some(image) = frame::render_image(drawn.pixels, width, height) else {
            return;
        };
        // The old image has to leave GPUI's atlas, or every redraw of a
        // canvas leaks one frame's worth of texture.
        if let Some(old) = self.canvas.take() {
            cx.drop_image(old.image, Some(window));
        }
        self.canvas = Some(Canvas {
            request: drawn.request,
            size: drawn.size,
            image,
            boxes: drawn.boxes,
        });
        cx.emit(CanvasUpdated);
    }
}
