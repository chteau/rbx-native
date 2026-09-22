//! The wheel and the keyboard over the canvas: pan, zoom, nudge, delete,
//! and Alt for the distance readout.

use gpui_kit::*;
use rbx_dom::Ref;

use super::super::super::Shell;
use crate::ui_canvas::{position_shift, shifted, udim2_text};

impl Shell {
    /// The wheel: pan, or with Ctrl (Cmd) held zoom about the pointer.
    pub(in crate::shell::ui_editor) fn canvas_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(16.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control || event.modifiers.platform {
            let at = self.panel_point(event.position);
            self.ui.view = self.ui.view.zoomed((dy * 0.0025).exp(), at);
        } else {
            // Shift turns a plain wheel sideways, as it does in a browser.
            let (dx, dy) = match event.modifiers.shift && dx == 0.0 {
                true => (dy, 0.0),
                false => (dx, dy),
            };
            self.ui.view.pan = [self.ui.view.pan[0] + dx, self.ui.view.pan[1] + dy];
        }
        self.ui.fitted = false;
        cx.notify();
    }

    /// Keys typed with the canvas focused: the arrows nudge the selection a
    /// pixel (ten with Shift), Delete removes it — through the Explorer's
    /// own delete. Undo, redo, save and group are the window's, and reach
    /// here like anywhere else.
    pub(in crate::shell::ui_editor) fn canvas_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let step = if keystroke.modifiers.shift { 10.0 } else { 1.0 };
        let nudge = match keystroke.key.as_str() {
            "left" => [-step, 0.0],
            "right" => [step, 0.0],
            "up" => [0.0, -step],
            "down" => [0.0, step],
            "delete" | "backspace" => {
                self.delete_selected(cx);
                return true;
            }
            _ => return false,
        };
        let Some((root, boxes)) = self.canvas_boxes(cx) else {
            return false;
        };
        let writes: Vec<(Ref, &str, String)> = self
            .held_selection(&root, &boxes)
            .iter()
            .map(|h| {
                let moved = position_shift(nudge, h.parent_rotation(), h.anchor, [0.0; 2]);
                (
                    h.referent,
                    "Position",
                    udim2_text(shifted(h.position, moved)),
                )
            })
            .collect();
        if writes.is_empty() {
            return false;
        }
        self.write_drag(true, &writes, cx);
        true
    }

    pub(in crate::shell::ui_editor) fn canvas_modifiers(
        &mut self,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if modifiers.alt != self.ui.measuring {
            self.ui.measuring = modifiers.alt;
            cx.notify();
        }
    }
}
