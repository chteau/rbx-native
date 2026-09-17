//! The wheel over a `ScrollingFrame` in the viewport moves its
//! `CanvasPosition` on the live DOM, the way Studio's edit view scrolls a
//! list without making an edit of it.

use gpui_kit::Context;
use rbx_dom::{Variant, Vector2Data};

use super::Shell;
use crate::workspace_view::{scrolled, Scroll};

const CANVAS_POSITION: &str = "CanvasPosition";

impl Shell {
    /// One wheel notch the render thread resolved onto a frame (see
    /// `workspace_view::scroll`): written straight to the DOM and reflected
    /// into the viewport so the next frame shows the canvas moved, never
    /// through `Shell::push_history` — like `sync_camera_pose`, a scroll is
    /// viewing, not editing, and a notch per undo step would bury the
    /// history under a single flick. The Properties panel reads the DOM on
    /// its next render, so a selected frame's `CanvasPosition` row follows.
    pub(super) fn scroll_canvas(&mut self, scroll: &Scroll, cx: &mut Context<Self>) {
        let current = match self
            .dom
            .get(scroll.referent)
            .and_then(|frame| frame.properties().get(CANVAS_POSITION))
        {
            Some(Variant::Vector2(position)) => [position.x, position.y],
            _ => [0.0; 2],
        };
        let next = scrolled(current, scroll);
        if next == current {
            return;
        }
        let position = Variant::Vector2(Vector2Data {
            x: next[0],
            y: next[1],
        });
        if let Err(err) = self
            .dom
            .set_property(scroll.referent, CANVAS_POSITION, position)
        {
            self.output.push_warning(&format!("viewport scroll: {err}"));
            return;
        }
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        cx.notify();
    }
}
