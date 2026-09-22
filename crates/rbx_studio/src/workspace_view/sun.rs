//! The view's half of the Sun tool (see `crate::sun`): each step of the
//! gesture goes to `Shell` as the ray under the cursor, and `Shell`'s answer
//! comes back as the readout and the guide the renderer draws — through the
//! same preview pass the Align tool's ghost boxes use.

use gpui_kit::{Context, Pixels, Point, SharedString};
use rbx_viewer::gizmo;

use crate::sun::Guide;

use super::gizmo::Drag;
use super::{readout, ViewportAction, WorkspaceView};

impl WorkspaceView {
    /// One step of the Sun tool's gesture. The readout follows the cursor
    /// straight away, still saying what `Shell` last answered until its next
    /// answer lands.
    pub(super) fn sun_step(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        first: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(ray) = self.cursor_ray(position, scale) else {
            return;
        };
        let text = self
            .drag_readout
            .take()
            .map(|(_, text)| text)
            .unwrap_or_default();
        let at = readout::position(position, self.viewport.get().origin, scale);
        self.drag_readout = Some((at, text));
        cx.emit(ViewportAction::Sun { ray, first });
    }

    /// `Shell`'s answer to a Sun step: what the readout says, and the guide
    /// to draw if the step has one.
    ///
    /// Ignored once the gesture is over: the release lets go before the last
    /// step's answer arrives, and drawing it then would leave a guide on
    /// screen with nothing left to clear it.
    pub(crate) fn show_sun(
        &mut self,
        guide: Option<Guide>,
        text: SharedString,
        cx: &mut Context<Self>,
    ) {
        if self.drag != Some(Drag::Sun) {
            return;
        }
        if let Some((_, shown)) = &mut self.drag_readout {
            *shown = text;
        }
        let boxes = match (guide, self.view) {
            (Some(guide), Some(pose)) => {
                guide.boxes(gizmo::arm_length(guide.from, pose, self.orthographic))
            }
            _ => Vec::new(),
        };
        self.pump.preview(boxes);
        cx.notify();
    }

    /// Lets go of whatever drag is held, and returns it, taking the Sun
    /// tool's guide off screen with it: nothing else would, once its
    /// gesture is over.
    pub(super) fn drop_drag(&mut self) -> Option<Drag> {
        let drag = self.drag.take();
        if drag == Some(Drag::Sun) {
            self.pump.preview(Vec::new());
        }
        drag
    }
}
