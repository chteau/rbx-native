//! The Animation Editor's model, apart from any window: the clip being
//! edited, the playhead, which keys are selected, and every edit a builder
//! makes to them, each one undoable.
//!
//! Like Roblox's own editor, it works on a copy of the clip and writes it to
//! a `KeyframeSequence` only when saved (`shell::animation`). Posing a rig
//! sixty times a second must not churn the place's undo history, and a
//! half-made animation should not land in the place by accident. Its own
//! undo stack covers the edits made in between.

pub(crate) mod demo;
pub(crate) mod graph_canvas;
pub(crate) mod place;
mod rows;

use rbx_animation::{Clip, Direction, Frame, Marker, PoseKey, Style};
use rbx_dom::Ref;

pub(crate) use rows::{rows, Row};

/// The deepest the editor's own undo goes.
const UNDO_DEPTH: usize = 100;

/// One selected key: a joint and the time of its keyframe.
pub(crate) type KeyId = (String, f32);

/// Keys being dragged along the timeline: the clip as it was when the drag
/// began, so every move re-applies from there and the gesture is one undo.
#[derive(Clone, Debug)]
struct Drag {
    origin: Clip,
    keys: Vec<KeyId>,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimEditor {
    pub(crate) clip: Clip,
    /// The `KeyframeSequence` the clip was loaded from and saves back to.
    pub(crate) source: Option<Ref>,
    pub(crate) time: f32,
    pub(crate) fps: u32,
    pub(crate) playing: bool,
    pub(crate) selected: Vec<KeyId>,
    /// The joint (by `Part1` name) the side panel's pose fields edit.
    pub(crate) joint: Option<String>,
    /// Edits since the clip was last loaded or saved.
    pub(crate) dirty: bool,
    undo: Vec<Clip>,
    redo: Vec<Clip>,
    drag: Option<Drag>,
    clipboard: Vec<(String, f32, PoseKey)>,
}

impl Default for AnimEditor {
    fn default() -> Self {
        AnimEditor {
            clip: Clip::default(),
            source: None,
            time: 0.0,
            fps: 30,
            playing: false,
            selected: Vec::new(),
            joint: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            clipboard: Vec::new(),
        }
    }
}

fn same_time(a: f32, b: f32) -> bool {
    (a - b).abs() < rbx_animation::clip::TIME_EPSILON
}

impl AnimEditor {
    /// Starts editing `clip`, forgetting the previous one's history.
    pub(crate) fn load(&mut self, clip: Clip, source: Option<Ref>) {
        self.clip = clip;
        self.source = source;
        self.time = 0.0;
        self.playing = false;
        self.selected.clear();
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.dirty = false;
    }

    /// Applies one undoable edit.
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut Clip)) {
        let before = self.clip.clone();
        change(&mut self.clip);
        if self.clip != before {
            self.undo.push(before);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.dirty = true;
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.clip, previous));
        self.dirty = true;
        self.prune_selection();
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.clip, next));
        self.dirty = true;
        self.prune_selection();
        true
    }

    fn prune_selection(&mut self) {
        let clip = &self.clip;
        self.selected.retain(|(j, t)| clip.pose(*t, j).is_some());
    }

    /// `time` on the editor's frame grid, never before 0.
    pub(crate) fn snap(&self, time: f32) -> f32 {
        let fps = self.fps.max(1) as f32;
        ((time * fps).round() / fps).max(0.0)
    }

    pub(crate) fn frame(&self) -> i64 {
        (self.time * self.fps.max(1) as f32).round() as i64
    }

    pub(crate) fn set_time(&mut self, time: f32) {
        self.time = self.snap(time);
    }

    /// Advances playback by `dt`. A looping clip wraps; any other stops at
    /// its last key. Returns false once playback has stopped.
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        if !self.playing {
            return false;
        }
        let length = self.clip.length();
        if length <= 0.0 {
            self.playing = false;
            return false;
        }
        self.time += dt;
        if self.time > length {
            if self.clip.looped {
                self.time = self.time.rem_euclid(length);
            } else {
                self.time = length;
                self.playing = false;
            }
        }
        self.playing
    }

    pub(crate) fn toggle_play(&mut self) {
        if !self.playing && self.time >= self.clip.length() {
            self.time = 0.0;
        }
        self.playing = !self.playing && self.clip.length() > 0.0;
    }

    pub(crate) fn step_frames(&mut self, frames: i64) {
        self.playing = false;
        let fps = self.fps.max(1) as f32;
        self.time = ((self.frame() + frames) as f32 / fps).max(0.0);
    }

    /// Jumps to the next (or previous) keyframe time from the playhead.
    pub(crate) fn jump_key(&mut self, forward: bool) {
        self.playing = false;
        let now = self.time;
        let times = self.clip.keyframes.iter().map(|k| k.time);
        let target = if forward {
            times.filter(|t| *t > now + 1e-4).reduce(f32::min)
        } else {
            times.filter(|t| *t < now - 1e-4).reduce(f32::max)
        };
        if let Some(t) = target {
            self.time = t;
        }
    }

    /// Keys `joint` at the playhead with `transform`, keeping the easing an
    /// existing key there already had.
    pub(crate) fn key(&mut self, joint: &str, transform: Frame) {
        let time = self.snap(self.time);
        self.edit(|clip| {
            let mut key = clip
                .pose(time, joint)
                .copied()
                .unwrap_or(PoseKey::new(transform));
            key.cframe = transform;
            clip.set_pose(time, joint, key);
        });
        self.selected = vec![(joint.to_owned(), time)];
    }

    /// Keys every joint given at the playhead, as one undo step.
    pub(crate) fn key_all(&mut self, poses: &[(String, Frame)]) {
        let time = self.snap(self.time);
        self.edit(|clip| {
            for (joint, transform) in poses {
                let mut key = clip
                    .pose(time, joint)
                    .copied()
                    .unwrap_or(PoseKey::new(*transform));
                key.cframe = *transform;
                clip.set_pose(time, joint, key);
            }
        });
        self.selected = poses.iter().map(|(j, _)| (j.clone(), time)).collect();
    }

    pub(crate) fn select(&mut self, key: KeyId, extend: bool) {
        if extend {
            if let Some(at) = self
                .selected
                .iter()
                .position(|k| k.0 == key.0 && same_time(k.1, key.1))
            {
                self.selected.remove(at);
                return;
            }
        } else {
            self.selected.clear();
        }
        self.joint = Some(key.0.clone());
        self.selected.push(key);
    }

    /// Selects every key of one keyframe (the summary row's diamond).
    pub(crate) fn select_keyframe(&mut self, time: f32, extend: bool) {
        if !extend {
            self.selected.clear();
        }
        if let Some(k) = self.clip.keyframes.iter().find(|k| same_time(k.time, time)) {
            self.selected
                .extend(k.poses.keys().map(|j| (j.clone(), k.time)));
        }
    }

    pub(crate) fn is_selected(&self, joint: &str, time: f32) -> bool {
        self.selected
            .iter()
            .any(|(j, t)| j == joint && same_time(*t, time))
    }

    pub(crate) fn delete_selected(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let keys = std::mem::take(&mut self.selected);
        self.edit(|clip| {
            for (joint, time) in &keys {
                clip.remove_pose(*time, joint);
            }
        });
        true
    }

    pub(crate) fn begin_drag(&mut self) {
        if !self.selected.is_empty() {
            self.drag = Some(Drag {
                origin: self.clip.clone(),
                keys: self.selected.clone(),
            });
        }
    }

    /// Moves the dragged keys `delta` seconds from where they started,
    /// snapped to frames and never before 0.
    pub(crate) fn drag_by(&mut self, delta: f32) {
        let Some(drag) = &self.drag else { return };
        let fps = self.fps.max(1) as f32;
        let shift = (delta * fps).round() / fps;
        let earliest = drag.keys.iter().map(|k| k.1).fold(f32::INFINITY, f32::min);
        let shift = shift.max(-earliest);
        let mut clip = drag.origin.clone();
        let moved: Vec<(String, f32, PoseKey)> = drag
            .keys
            .iter()
            .filter_map(|(j, t)| clip.remove_pose(*t, j).map(|k| (j.clone(), *t + shift, k)))
            .collect();
        for (joint, time, key) in &moved {
            clip.set_pose(*time, joint, *key);
        }
        self.clip = clip;
        self.selected = moved.into_iter().map(|(j, t, _)| (j, t)).collect();
    }

    pub(crate) fn end_drag(&mut self) {
        if let Some(drag) = self.drag.take() {
            if drag.origin != self.clip {
                self.undo.push(drag.origin);
                self.redo.clear();
                self.dirty = true;
            }
        }
    }

    /// Sets the easing of every selected key (either half left as it is
    /// when `None`).
    pub(crate) fn set_easing(&mut self, style: Option<Style>, direction: Option<Direction>) {
        let keys = self.selected.clone();
        self.edit(|clip| {
            for (joint, time) in &keys {
                if let Some(key) = clip.pose_mut(*time, joint) {
                    if let Some(s) = style {
                        key.style = s;
                    }
                    if let Some(d) = direction {
                        key.direction = d;
                    }
                }
            }
        });
    }

    /// The easing every selected key shares, if they agree.
    pub(crate) fn easing(&self) -> Option<(Style, Direction)> {
        let mut found = self
            .selected
            .iter()
            .filter_map(|(j, t)| self.clip.pose(*t, j))
            .map(|k| (k.style, k.direction));
        let first = found.next()?;
        found.all(|e| e == first).then_some(first)
    }

    pub(crate) fn copy(&mut self) -> usize {
        let earliest = self
            .selected
            .iter()
            .map(|k| k.1)
            .fold(f32::INFINITY, f32::min);
        self.clipboard = self
            .selected
            .iter()
            .filter_map(|(j, t)| self.clip.pose(*t, j).map(|k| (j.clone(), t - earliest, *k)))
            .collect();
        self.clipboard.len()
    }

    /// Pastes what was copied with its earliest key at the playhead.
    pub(crate) fn paste(&mut self) -> bool {
        if self.clipboard.is_empty() {
            return false;
        }
        let at = self.snap(self.time);
        let keys = self.clipboard.clone();
        self.edit(|clip| {
            for (joint, offset, key) in &keys {
                clip.set_pose(at + offset, joint, *key);
            }
        });
        self.selected = keys.iter().map(|(j, o, _)| (j.clone(), at + o)).collect();
        true
    }

    /// Adds an animation event at the playhead.
    pub(crate) fn add_marker(&mut self, name: &str, value: &str) {
        let time = self.snap(self.time);
        let marker = Marker {
            name: name.to_owned(),
            value: value.to_owned(),
        };
        self.edit(|clip| clip.keyframe_mut(time).markers.push(marker));
    }

    pub(crate) fn remove_markers_here(&mut self) -> bool {
        let time = self.snap(self.time);
        let had = self
            .clip
            .keyframes
            .iter()
            .any(|k| same_time(k.time, time) && !k.markers.is_empty());
        self.edit(|clip| {
            if let Some(i) = clip.keyframe_index(time) {
                clip.keyframes[i].markers.clear();
                if clip.keyframes[i].poses.is_empty() {
                    clip.keyframes.remove(i);
                }
            }
        });
        had
    }
}

#[cfg(test)]
mod tests;
