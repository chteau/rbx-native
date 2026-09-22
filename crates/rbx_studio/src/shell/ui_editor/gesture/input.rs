//! The wheel and the keyboard over the canvas: pan, zoom, nudge, delete,
//! the drawing tools' keys, paint order, and Alt for the distance readout.

use gpui_kit::*;
use rbx_dom::Ref;

use super::super::super::Shell;
use super::super::draw::TOOL_KEYS;
use super::super::order::Arrange;
use crate::ui_canvas::{position_shift, shifted_in, udim2_text, View};

/// Room left round the selection when the canvas zooms to it.
const ZOOM_MARGIN: f32 = 64.0;

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
        // Keys typed into the text field over an element are the field's.
        if self.ui.text_edit.is_some() {
            if keystroke.key == "escape" {
                self.end_text_edit(false, cx);
                return true;
            }
            return false;
        }
        // Space held turns a drag on the canvas into a pan.
        if keystroke.key == "space" {
            self.ui.panning = true;
            return true;
        }
        let bare = !(keystroke.modifiers.control
            || keystroke.modifiers.platform
            || keystroke.modifiers.alt
            || keystroke.modifiers.shift);
        if let Some(&(_, class)) = TOOL_KEYS
            .iter()
            .find(|(key, _)| bare && *key == keystroke.key.as_str())
        {
            self.arm_tool(class, cx);
            return true;
        }
        if matches!(keystroke.key.as_str(), "escape" | "v") && bare && self.ui.tool.is_some() {
            self.ui.tool = None;
            cx.notify();
            return true;
        }
        if keystroke.key == "escape" && bare {
            self.deselect(cx);
            return true;
        }
        if keystroke.modifiers.control || keystroke.modifiers.platform {
            let shift = keystroke.modifiers.shift;
            let arrange = match keystroke.key.as_str() {
                "]" | "}" if shift => Some(Arrange::Front),
                "]" | "}" => Some(Arrange::Forward),
                "[" | "{" if shift => Some(Arrange::Back),
                "[" | "{" => Some(Arrange::Backward),
                _ => None,
            };
            if let Some(arrange) = arrange {
                self.arrange_gui(arrange, cx);
                return true;
            }
            match keystroke.key.as_str() {
                "0" => {
                    self.ui.fitted = true;
                    cx.notify();
                    return true;
                }
                "1" => {
                    self.zoom_to_selection(cx);
                    return true;
                }
                _ => {}
            }
        }
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
                let position = shifted_in(h.position, moved, self.ui.unit, h.position_span());
                (h.referent, "Position", udim2_text(position))
            })
            .collect();
        if writes.is_empty() {
            return false;
        }
        self.write_drag(true, &writes, cx);
        true
    }

    /// Ctrl+1: the selection's frame filling the panel.
    pub(in crate::shell::ui_editor) fn zoom_to_selection(&mut self, cx: &mut Context<Self>) {
        let Some((_, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let Some((rect, turn)) = self.selection_frame(&boxes) else {
            return;
        };
        let panel = self.ui.bounds.get().size;
        let panel = [f32::from(panel.width), f32::from(panel.height)];
        self.ui.view = View::framing(panel, &rect.turned_bounds(turn), ZOOM_MARGIN);
        self.ui.fitted = false;
        cx.notify();
    }

    pub(in crate::shell::ui_editor) fn canvas_key_up(&mut self, keystroke: &Keystroke) {
        if keystroke.key == "space" {
            self.ui.panning = false;
        }
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
