//! The dope sheet's pointer and keyboard handling, and the actions they
//! share with the panel's buttons.

use gpui_kit::*;

use super::super::Gesture;
use super::{Shell, ROW};

impl Shell {
    pub(in crate::shell::animation) fn focus_channel(
        &mut self,
        joint: &str,
        cx: &mut Context<Self>,
    ) {
        self.anim.editor.joint = Some(joint.to_owned());
        let part = self
            .anim
            .bound
            .as_ref()
            .and_then(|b| b.rig.joint_for(joint))
            .map(|j| j.part1);
        if let Some(part) = part {
            self.select(part, cx);
        }
        cx.notify();
    }

    pub(in crate::shell::animation) fn sheet_press(
        &mut self,
        at: Point<Pixels>,
        shift: bool,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let x = f32::from(at.x - b.origin.x);
        let y = f32::from(at.y - b.origin.y);
        let row = (y / ROW).floor() as isize;
        let rows = self.sheet_rows();
        let tl = self.anim.timeline;
        self.anim.editor.playing = false;
        if row == 0 {
            let times: Vec<f32> = self
                .anim
                .editor
                .clip
                .keyframes
                .iter()
                .map(|k| k.time)
                .collect();
            if let Some(i) = tl.hit(&times, x) {
                self.anim.editor.select_keyframe(times[i], shift);
                self.anim.editor.begin_drag();
                self.anim.gesture = Some(Gesture::MoveKeys { from_x: x });
                cx.notify();
                return;
            }
        } else if let Some(r) = rows.get((row - 1).max(0) as usize).filter(|_| row >= 1) {
            let times: Vec<f32> = crate::anim_editor::keys::times(&self.anim.editor.clip, &r.name);
            if let Some(i) = tl.hit(&times, x) {
                let key = (r.name.clone(), times[i]);
                if !self.anim.editor.is_selected(&key.0, key.1) || shift {
                    self.anim.editor.select(key, shift);
                }
                self.anim.editor.joint = Some(r.name.clone());
                self.anim.editor.begin_drag();
                self.anim.gesture = Some(Gesture::MoveKeys { from_x: x });
                cx.notify();
                return;
            }
            if clicks >= 2 {
                let name = r.name.clone();
                self.anim.editor.set_time(tl.time_at(x));
                match crate::anim_editor::keys::face_channel(&name) {
                    Some(channel) => {
                        let e = &mut self.anim.editor;
                        let at = rbx_animation::sample::wrap(&e.clip, e.time);
                        let value =
                            rbx_animation::sample::sample_face(&e.clip, channel, at).unwrap_or(0.0);
                        let channel = channel.to_owned();
                        e.key_face(&channel, value, false);
                        cx.notify();
                    }
                    None => self.key_joints(Some(&name), cx),
                }
                return;
            }
            self.anim.editor.joint = Some(r.name.clone());
        }
        if !shift {
            self.anim.editor.selected.clear();
        }
        self.anim.editor.set_time(tl.time_at(x));
        self.anim.gesture = Some(Gesture::Scrub);
        cx.notify();
    }

    pub(in crate::shell::animation) fn sheet_drag(
        &mut self,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(gesture) = self.anim.gesture else {
            return;
        };
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let x = f32::from(at.x - b.origin.x);
        match gesture {
            Gesture::Scrub => {
                let t = self.anim.timeline.time_at(x);
                self.anim.editor.set_time(t);
            }
            Gesture::MoveKeys { from_x } => {
                let tl = self.anim.timeline;
                self.anim.editor.drag_by(tl.time_at(x) - tl.time_at(from_x));
            }
            Gesture::Pan { last_x } => {
                let now = f32::from(at.x);
                self.anim.timeline.pan(now - last_x);
                self.anim.gesture = Some(Gesture::Pan { last_x: now });
            }
            Gesture::CurveKeys { .. } | Gesture::Tangent { .. } => {
                let y = f32::from(at.y - b.origin.y);
                self.curve_drag(gesture, x, y);
            }
        }
        cx.notify();
    }

    pub(in crate::shell::animation) fn sheet_release(&mut self, cx: &mut Context<Self>) {
        if self.anim.gesture.take().is_some() {
            self.anim.editor.end_drag();
            self.anim.editor.end_curve_drag();
            cx.notify();
        }
    }

    pub(in crate::shell::animation) fn sheet_wheel(
        &mut self,
        e: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(b) = self.anim.sheet.get() else {
            return;
        };
        let delta = e.delta.pixel_delta(window.line_height());
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if e.modifiers.shift || dx.abs() > dy.abs() {
            self.anim.timeline.pan(if dx != 0.0 { dx } else { dy });
        } else if e.modifiers.control || e.modifiers.platform {
            let x = f32::from(e.position.x - b.origin.x);
            self.anim.timeline.zoom_about(x, (dy * 0.01).exp());
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Keys on the dope sheet, Blender's where they exist: Space plays,
    /// I inserts a key, X or Delete removes, arrows step frames (Up/Down
    /// jump between keys), Home/End go to the ends, A selects every key,
    /// Alt+A or Escape none, F fits the clip, Ctrl+C/V copy and paste.
    pub(in crate::shell::animation) fn sheet_key(
        &mut self,
        key: &Keystroke,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let m = &key.modifiers;
        let ctrl = m.control || m.platform;
        if self.anim.mode == super::super::Mode::Curves
            && self.anim.editor.curves.is_some()
            && !ctrl
            && self.curve_key_press(key.key.as_str(), m.alt)
        {
            cx.notify();
            return true;
        }
        let e = &mut self.anim.editor;
        match key.key.as_str() {
            "space" => e.toggle_play(),
            "i" if !ctrl => {
                let joint = e.joint.clone();
                self.key_joints(joint.as_deref(), cx);
                return true;
            }
            "x" | "delete" | "backspace" if !ctrl => {
                e.delete_selected();
            }
            "left" => e.step_frames(-1),
            "right" => e.step_frames(1),
            "up" => e.jump_key(true),
            "down" => e.jump_key(false),
            "home" => e.set_time(0.0),
            "end" => {
                let length = e.clip.length();
                e.set_time(length);
            }
            "a" if m.alt => e.selected.clear(),
            "a" if !ctrl => {
                let all = e
                    .clip
                    .keyframes
                    .iter()
                    .flat_map(|k| k.poses.keys().map(move |j| (j.clone(), k.time)))
                    .collect();
                e.selected = all;
            }
            "escape" => e.selected.clear(),
            "f" if !ctrl => self.fit_timeline(),
            "c" if ctrl => {
                e.copy();
            }
            "v" if ctrl => {
                e.paste();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Fits the timeline to the clip's length (or a second, for an empty
    /// one) across the sheet's current width.
    pub(in crate::shell::animation) fn fit_timeline(&mut self) {
        let width = self
            .anim
            .sheet
            .get()
            .map_or(800.0, |b| f32::from(b.size.width));
        let length = self.anim.editor.clip.length().max(1.0);
        self.anim.timeline.fps = self.anim.editor.fps;
        self.anim.timeline.fit(length, width);
        self.anim.fitted = true;
    }

    /// Keys `joint` (or every joint the rig has, with `None`) at the
    /// playhead in the pose it shows now.
    pub(in crate::shell::animation) fn key_joints(
        &mut self,
        joint: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let current = self.current_transforms();
        let names: Vec<String> = match (joint, &self.anim.bound) {
            (Some(j), _) => vec![j.to_owned()],
            (None, Some(b)) => b
                .rig
                .joints
                .iter()
                .map(|j| b.rig.part_name(j.part1).to_owned())
                .collect(),
            (None, None) => self.anim.editor.clip.joints(),
        };
        let poses: Vec<(String, rbx_animation::Frame)> = names
            .into_iter()
            .map(|n| {
                let f = current.get(&n).copied().unwrap_or_default();
                (n, f)
            })
            .collect();
        self.anim.editor.key_all(&poses);
        cx.notify();
    }
}
