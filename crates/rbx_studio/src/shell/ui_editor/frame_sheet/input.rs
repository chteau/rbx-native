//! The stage's pointer and keys: the free camera on the right button, WASD
//! and the wheel (the 3D view's own controls, through `rbx_viewer::Flight`),
//! the Orbit or Hand tool on the left button, and the sheet's shortcuts.

use std::time::{Duration, Instant};

use glam::Vec3;
use gpui_kit::*;
use rbx_viewer::CameraInput;

use super::{Popover, Shell, Tool};
use crate::viewport_frame as frame;
use crate::workspace_view::input::{camera_key, chorded, wheel_notches, Layout};

/// Degrees the Orbit tool turns per pixel dragged — the free camera's own
/// mouse-look rate, so the two feel alike.
const ORBIT_DEGREES_PER_PIXEL: f32 = 0.3;

/// The longest step a flight takes in one go: a frame that came round late
/// (a hitch, a debugger) must not fling the camera across the scene.
const MAX_STEP: Duration = Duration::from_millis(100);

impl Shell {
    pub(super) fn frame_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        window.focus(&sheet.focus, cx);
        sheet.popover = None;
        sheet.pointer = Some(event.position);
        match event.button {
            MouseButton::Right => {
                sheet.flight.input(CameraInput::LookButton(true));
                self.fly(window, cx);
            }
            MouseButton::Left => sheet.dragging = true,
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn frame_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        match event.button {
            MouseButton::Right => {
                sheet.flight.input(CameraInput::LookButton(false));
                self.fly(window, cx);
            }
            MouseButton::Left if sheet.dragging => {
                sheet.dragging = false;
                sheet.gesture.close();
            }
            _ => {}
        }
    }

    pub(super) fn frame_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        let Some(previous) = sheet.pointer.replace(event.position) else {
            return;
        };
        let (dx, dy) = (
            f32::from(event.position.x - previous.x),
            f32::from(event.position.y - previous.y),
        );
        if event.pressed_button == Some(MouseButton::Right) {
            sheet.flight.input(CameraInput::MouseLook { dx, dy });
            self.fly(window, cx);
            return;
        }
        if !sheet.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let (target, tool, height) = (
            sheet.frame,
            sheet.tool,
            f32::from(sheet.area.get().size.height).max(1.0),
        );
        let pose = self.frame_pose(target, cx);
        let pivot = frame::contents(&self.dom, &self.database, target)
            .map_or(Vec3::ZERO, |(centre, _)| centre);
        let moved = match tool {
            // Dragging right turns the scene right, as Studio's own orbit.
            Tool::Orbit => frame::orbit(
                pose,
                pivot,
                -(dx * ORBIT_DEGREES_PER_PIXEL).to_radians(),
                (dy * ORBIT_DEGREES_PER_PIXEL).to_radians(),
            ),
            // The point under the pointer stays under it: a pixel at the
            // pivot's depth is this many studs across.
            Tool::Hand => {
                let depth = pose.position.distance(pivot);
                let studs = 2.0 * depth * (pose.fov_degrees.to_radians() * 0.5).tan() / height;
                frame::pan(pose, -dx * studs, dy * studs)
            }
        };
        let step = self
            .ui
            .sheet
            .as_mut()
            .and_then(|sheet| sheet.gesture.step(true, true));
        if let Some(step) = step {
            self.write_frame_pose(moved, step, cx);
        }
    }

    pub(super) fn frame_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        let notches = wheel_notches(event.delta);
        if notches != 0.0 {
            sheet.flight.input(CameraInput::Wheel { notches });
            self.fly(window, cx);
        }
    }

    /// The sheet's keys: the camera's movement keys, its shortcuts, and
    /// Escape — which closes a popover first, then the sheet.
    pub(super) fn frame_key(
        &mut self,
        keystroke: &Keystroke,
        pressed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(sheet) = &mut self.ui.sheet else {
            return false;
        };
        // A field being typed in — the search, a lighting field, the field
        // of view — keeps its keys, all but the one that backs out of it.
        let typing = !sheet.focus.is_focused(window);
        if typing && keystroke.key != "escape" {
            return false;
        }
        let layout = Layout::of(cx.keyboard_layout().name());
        if pressed && chorded(keystroke.modifiers) {
            return false;
        }
        if pressed {
            match keystroke.key.as_str() {
                "escape" if sheet.popover.is_some() => {
                    sheet.popover = None;
                    cx.notify();
                    return true;
                }
                "escape" => {
                    self.close_frame_sheet(window, cx);
                    return true;
                }
                "f" => {
                    self.fit_frame(cx);
                    return true;
                }
                "home" => {
                    self.reset_frame_camera(cx);
                    return true;
                }
                "t" => {
                    self.top_frame_view(cx);
                    return true;
                }
                _ => {}
            }
        }
        let Some(key) = camera_key(&keystroke.key, layout) else {
            return false;
        };
        sheet.flight.input(CameraInput::Key { key, pressed });
        self.fly(window, cx);
        true
    }

    /// Steps the flight once now and again every frame while anything
    /// steers it, writing each step that moves the pose.
    fn fly(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        // A step already waits for the next frame; it reads this input too.
        if sheet
            .ticked
            .is_some_and(|ticked| ticked.elapsed() < MAX_STEP)
            && sheet.flight.busy()
        {
            return;
        }
        self.fly_step(window, cx);
    }

    fn fly_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.ui.sheet.as_ref().map(|sheet| sheet.frame) else {
            return;
        };
        let start = self.frame_pose(target, cx);
        let Some(sheet) = &mut self.ui.sheet else {
            return;
        };
        let now = Instant::now();
        let dt = sheet
            .ticked
            .map_or(Duration::from_millis(16), |ticked| now - ticked)
            .min(MAX_STEP);
        let mut pose = start;
        sheet.flight.step(&mut pose, dt);
        let busy = sheet.flight.busy();
        sheet.ticked = busy.then_some(now);
        let moved =
            (pose.position, pose.yaw, pose.pitch) != (start.position, start.yaw, start.pitch);
        if let Some(step) = sheet.gesture.step(moved, busy) {
            self.write_frame_pose(pose, step, cx);
        }
        if busy {
            cx.on_next_frame(window, |shell, window, cx| shell.fly_step(window, cx));
        }
    }

    /// Toggles a popover, closing the other.
    pub(super) fn toggle_frame_popover(&mut self, popover: Popover, cx: &mut Context<Self>) {
        if let Some(sheet) = &mut self.ui.sheet {
            sheet.popover = match sheet.popover == Some(popover) {
                true => None,
                false => Some(popover),
            };
            cx.notify();
        }
    }
}
