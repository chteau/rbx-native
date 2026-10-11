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
pub(crate) mod draft;
pub(crate) mod graph_canvas;
pub(crate) mod keys;
pub(crate) mod place;
mod curves;
mod playback;
mod rows;

use rbx_animation::curve::{self, CurveClip};
use rbx_animation::{Clip, Direction, Frame, Marker, PoseKey, Style};
use rbx_dom::Ref;

pub(crate) use curves::CurveKeyId;
pub(crate) use rows::{rows, Row};

/// The deepest the editor's own undo goes.
const UNDO_DEPTH: usize = 100;

/// One selected key: a joint and the time of its keyframe.
pub(crate) type KeyId = (String, f32);

/// What one undo step restores: the clip, and its curves while the Curve
/// Editor has converted it.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    clip: Clip,
    curves: Option<CurveClip>,
}

/// Keys being dragged along the timeline: the clip as it was when the drag
/// began, so every move re-applies from there and the gesture is one undo.
#[derive(Clone, Debug)]
struct Drag {
    origin: Snapshot,
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
    /// The clip as curves, once the Curve Editor has converted it. While
    /// set, these are what the editor plays and edits; `clip` is kept baked
    /// from them so the dope sheet and Save see the same motion.
    pub(crate) curves: Option<CurveClip>,
    /// Curve keys selected in the Curve Editor.
    pub(crate) curve_selected: Vec<CurveKeyId>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag: Option<Drag>,
    curve_drag: Option<curves::CurveDrag>,
    clipboard: Vec<(String, f32, keys::AnyKey)>,
    /// The clip before a face slider's drag began, so the drag is one undo.
    face_drag: Option<Snapshot>,
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
            curves: None,
            curve_selected: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            curve_drag: None,
            clipboard: Vec::new(),
            face_drag: None,
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
        self.curves = None;
        self.curve_selected.clear();
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.curve_drag = None;
        self.dirty = false;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            clip: self.clip.clone(),
            curves: self.curves.clone(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) -> Snapshot {
        let now = self.snapshot();
        self.clip = snapshot.clip;
        self.curves = snapshot.curves;
        now
    }

    /// Records `before` as one undo step if anything changed since.
    fn commit(&mut self, before: Snapshot) {
        if before != self.snapshot() {
            self.undo.push(before);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.dirty = true;
        }
    }

    /// Applies one undoable edit to the clip. With curves present, a change
    /// to the clip's settings, markers or face keys is copied onto them, and
    /// any change to its poses converts the clip to curves afresh.
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut Clip)) {
        let before = self.snapshot();
        change(&mut self.clip);
        if self.clip != before.clip {
            let fps = self.fps.max(1) as f32;
            if let Some(curves) = &mut self.curves {
                if curves::same_poses(&self.clip, &before.clip) {
                    curves::copy_settings(&self.clip, curves);
                } else {
                    *curves = curve::from_clip(&self.clip, fps);
                }
            }
        }
        self.commit(before);
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
        let now = self.restore(previous);
        self.redo.push(now);
        self.dirty = true;
        self.prune_selection();
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let now = self.restore(next);
        self.undo.push(now);
        self.dirty = true;
        self.prune_selection();
        true
    }

    fn prune_selection(&mut self) {
        let clip = &self.clip;
        self.selected
            .retain(|(j, t)| keys::get(clip, j, *t).is_some());
        self.prune_curve_selection();
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

    /// Keys `joint` at the playhead with `transform`, keeping the easing an
    /// existing key there already had.
    pub(crate) fn key(&mut self, joint: &str, transform: Frame) {
        let time = self.snap(self.time);
        if self.curves.is_some() {
            self.key_curves(&[(joint.to_owned(), transform)]);
            self.selected = vec![(joint.to_owned(), time)];
            return;
        }
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
        if self.curves.is_some() {
            self.key_curves(poses);
            self.selected = poses.iter().map(|(j, _)| (j.clone(), time)).collect();
            return;
        }
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
        if let Some(i) = self.clip.keyframe_index(time) {
            self.selected.extend(keys::on_keyframe(&self.clip, i));
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
        let picked = std::mem::take(&mut self.selected);
        self.edit(|clip| {
            for (name, time) in &picked {
                keys::take(clip, name, *time);
            }
        });
        true
    }

    pub(crate) fn begin_drag(&mut self) {
        if !self.selected.is_empty() {
            self.drag = Some(Drag {
                origin: self.snapshot(),
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
        let mut clip = drag.origin.clip.clone();
        let moved: Vec<(String, f32, keys::AnyKey)> = drag
            .keys
            .iter()
            .filter_map(|(n, t)| keys::take(&mut clip, n, *t).map(|k| (n.clone(), *t + shift, k)))
            .collect();
        for (name, time, key) in &moved {
            keys::put(&mut clip, name, *time, *key);
        }
        self.clip = clip;
        self.selected = moved.into_iter().map(|(n, t, _)| (n, t)).collect();
    }

    pub(crate) fn end_drag(&mut self) {
        if let Some(drag) = self.drag.take() {
            if self.curves.is_some() && drag.origin.clip != self.clip {
                self.curves = Some(curve::from_clip(&self.clip, self.fps.max(1) as f32));
            }
            self.commit(drag.origin);
        }
    }

    /// Sets the easing of every selected key (either half left as it is
    /// when `None`).
    pub(crate) fn set_easing(&mut self, style: Option<Style>, direction: Option<Direction>) {
        let picked = self.selected.clone();
        self.edit(|clip| {
            for (name, time) in &picked {
                keys::set_easing(clip, name, *time, style, direction);
            }
        });
    }

    /// The easing every selected key shares, if they agree.
    pub(crate) fn easing(&self) -> Option<(Style, Direction)> {
        let mut found = self
            .selected
            .iter()
            .filter_map(|(n, t)| keys::get(&self.clip, n, *t))
            .map(|k| k.easing());
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
            .filter_map(|(n, t)| keys::get(&self.clip, n, *t).map(|k| (n.clone(), t - earliest, k)))
            .collect();
        self.clipboard.len()
    }

    /// Pastes what was copied with its earliest key at the playhead.
    pub(crate) fn paste(&mut self) -> bool {
        if self.clipboard.is_empty() {
            return false;
        }
        let at = self.snap(self.time);
        let copied = self.clipboard.clone();
        self.edit(|clip| {
            for (name, offset, key) in &copied {
                keys::put(clip, name, at + offset, *key);
            }
        });
        self.selected = copied.iter().map(|(n, o, _)| (n.clone(), at + o)).collect();
        true
    }

    /// Keys a face channel at the playhead. During a slider drag (`live`)
    /// every step rewrites the same key and the whole drag is one undo.
    pub(crate) fn key_face(&mut self, channel: &str, value: f32, live: bool) {
        let time = self.snap(self.time);
        let write = |clip: &mut Clip| {
            let mut key = clip
                .keyframe_index(time)
                .and_then(|i| clip.keyframes[i].faces.get(channel).copied())
                .unwrap_or(rbx_animation::facs::FaceKey::new(value));
            key.value = value.clamp(0.0, 1.0);
            clip.set_face(time, channel, key);
        };
        if live {
            if self.face_drag.is_none() {
                self.face_drag = Some(self.snapshot());
            }
            write(&mut self.clip);
            self.dirty = true;
        } else {
            self.edit(write);
        }
        self.selected = vec![(keys::face_row(channel), time)];
    }

    /// Ends a face slider's drag, recording it as one undo step.
    pub(crate) fn end_face_drag(&mut self) {
        if let Some(origin) = self.face_drag.take() {
            self.commit(origin);
        }
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
