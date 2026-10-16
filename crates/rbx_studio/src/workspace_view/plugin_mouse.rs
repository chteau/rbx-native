//! The 3D view's mouse while a plugin is activated: Studio hands presses,
//! moves and the wheel to that plugin's `PluginMouse` (`Button1Down`,
//! `Move`, …) and stops selecting, which is how building plugins such as
//! F3X or Brushtool take over the viewport.

use gpui_kit::*;
use rbx_lua::plugin::MouseInput;

use super::{ViewportAction, WorkspaceView};

impl WorkspaceView {
    /// `position` in the view's own logical pixels, as `Mouse.X`/`Y` and
    /// `UserInputService:GetMouseLocation` count them.
    fn view_point(&self, position: Point<Pixels>, scale: f32) -> [f32; 2] {
        let origin = self.viewport.get().origin;
        let scale = scale.max(0.01);
        [
            f32::from(position.x) - origin.0 as f32 / scale,
            f32::from(position.y) - origin.1 as f32 / scale,
        ]
    }

    fn emit_mouse(&self, input: MouseInput, at: [f32; 2], cx: &mut Context<Self>) {
        cx.emit(ViewportAction::PluginMouse(input, at));
    }

    /// Whether a plugin owns the mouse; the shell keeps this in step with
    /// `PluginHost::active_plugin`.
    pub(crate) fn set_plugin_mouse(&mut self, owned: bool) {
        self.plugin_handles.mouse_owned = owned;
        if !owned {
            self.plugin_handles.mouse_held = false;
        }
    }

    /// A press while a plugin owns the mouse: `true` when it was the
    /// plugin's (the left button, which then selects nothing). The right
    /// button is reported but still turns the camera.
    pub(super) fn plugin_mouse_press(
        &mut self,
        button: u8,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.plugin_handles.mouse_owned {
            return false;
        }
        let at = self.view_point(position, scale);
        self.emit_mouse(MouseInput::Down(button), at, cx);
        if button == 1 {
            self.plugin_handles.mouse_held = true;
        }
        button == 1
    }

    /// A release ending a press the plugin took.
    pub(super) fn plugin_mouse_release(
        &mut self,
        button: u8,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.plugin_handles.mouse_owned {
            return false;
        }
        let at = self.view_point(position, scale);
        self.emit_mouse(MouseInput::Up(button), at, cx);
        if button == 1 {
            return std::mem::take(&mut self.plugin_handles.mouse_held);
        }
        false
    }

    pub(super) fn plugin_mouse_move(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_handles.mouse_owned {
            let at = self.view_point(position, scale);
            self.emit_mouse(MouseInput::Move, at, cx);
        }
    }

    pub(super) fn plugin_mouse_wheel(
        &mut self,
        delta: f32,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_handles.mouse_owned && delta != 0.0 {
            let dir = if delta > 0.0 { 1 } else { -1 };
            let at = self.view_point(position, scale);
            self.emit_mouse(MouseInput::Wheel(dir), at, cx);
        }
    }
}
