//! The Curve Editor's half of the model: converting the clip to curves and
//! back, and every edit made to a curve key. Curves, once made, are what the
//! editor edits; after each edit the clip is baked from them again
//! (`curve::to_clip`), so the dope sheet, playback and Save all see the
//! motion the curves describe. Face keys are not curves and stay on the
//! clip as they were.

use rbx_animation::curve::{self, Channel, CurveClip, FloatCurve, FloatKey, Interpolation};
use rbx_animation::{Clip, Direction, Frame, Style};

use super::{same_time, AnimEditor, Snapshot};

/// One curve key: a joint's channel and the key's time on it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurveKeyId {
    pub(crate) joint: String,
    pub(crate) channel: Channel,
    pub(crate) time: f32,
}

/// Curve keys being dragged: where each started, so every step re-applies
/// from there and the gesture is one undo.
#[derive(Clone, Debug)]
pub(super) struct CurveDrag {
    origin: Snapshot,
    keys: Vec<(CurveKeyId, FloatKey)>,
}

/// Whether two clips pose every joint the same way at the same times, which
/// is all curves are made from.
pub(super) fn same_poses(a: &Clip, b: &Clip) -> bool {
    let posed = |c: &Clip| {
        c.keyframes
            .iter()
            .filter(|k| !k.poses.is_empty())
            .map(|k| (k.time, k.poses.clone()))
            .collect::<Vec<_>>()
    };
    posed(a) == posed(b)
}

/// Copies what a clip carries besides its poses onto its curves.
pub(super) fn copy_settings(clip: &Clip, curves: &mut CurveClip) {
    curves.name = clip.name.clone();
    curves.looped = clip.looped;
    curves.priority = clip.priority;
    curves.markers = clip
        .keyframes
        .iter()
        .flat_map(|k| k.markers.iter().map(|m| (k.time, m.clone())))
        .collect();
}

fn find(curve: &FloatCurve, time: f32) -> Option<usize> {
    curve.keys.iter().position(|k| same_time(k.time, time))
}

impl AnimEditor {
    fn bake_fps(&self) -> f32 {
        self.fps.max(1) as f32
    }

    /// Re-bakes the clip from the curves, keeping its face keys and
    /// keyframe names.
    fn bake(&mut self) {
        let Some(curves) = &self.curves else { return };
        let mut clip = curve::to_clip(curves, self.bake_fps());
        for old in &self.clip.keyframes {
            if !old.faces.is_empty() || (old.name != "Keyframe" && !old.poses.is_empty()) {
                let k = clip.keyframe_mut(old.time);
                k.faces
                    .extend(old.faces.iter().map(|(c, f)| (c.clone(), *f)));
                if old.name != "Keyframe" {
                    k.name = old.name.clone();
                }
            }
        }
        self.clip = clip;
    }

    /// One undoable edit to the curves, after which the clip is re-baked.
    pub(crate) fn edit_curves(&mut self, change: impl FnOnce(&mut CurveClip)) {
        let Some(mut curves) = self.curves.clone() else {
            return;
        };
        let before = self.snapshot();
        change(&mut curves);
        if Some(&curves) != before.curves.as_ref() {
            self.curves = Some(curves);
            self.bake();
        }
        self.commit(before);
        self.prune_curve_selection();
    }

    /// Converts the clip to curves (the editor's "Convert to Curves").
    pub(crate) fn convert_to_curves(&mut self) -> bool {
        if self.curves.is_some() {
            return false;
        }
        let before = self.snapshot();
        self.curves = Some(curve::from_clip(&self.clip, self.bake_fps()));
        self.commit(before);
        true
    }

    /// Goes back to keyframes: the clip stays as last baked.
    pub(crate) fn back_to_keyframes(&mut self) -> bool {
        if self.curves.is_none() {
            return false;
        }
        let before = self.snapshot();
        self.curves = None;
        self.curve_selected.clear();
        self.commit(before);
        true
    }

    pub(crate) fn curve_key(&self, id: &CurveKeyId) -> Option<FloatKey> {
        let curve = self
            .curves
            .as_ref()?
            .joints
            .get(&id.joint)?
            .curve(id.channel);
        find(curve, id.time).map(|i| curve.keys[i])
    }

    pub(super) fn prune_curve_selection(&mut self) {
        let picked = std::mem::take(&mut self.curve_selected);
        self.curve_selected = picked
            .into_iter()
            .filter(|id| self.curve_key(id).is_some())
            .collect();
    }

    pub(crate) fn is_curve_selected(&self, id: &CurveKeyId) -> bool {
        self.curve_selected
            .iter()
            .any(|k| k.joint == id.joint && k.channel == id.channel && same_time(k.time, id.time))
    }

    pub(crate) fn select_curve_key(&mut self, id: CurveKeyId, extend: bool) {
        if extend {
            if let Some(at) = self.curve_selected.iter().position(|k| {
                k.joint == id.joint && k.channel == id.channel && same_time(k.time, id.time)
            }) {
                self.curve_selected.remove(at);
                return;
            }
        } else {
            self.curve_selected.clear();
        }
        self.joint = Some(id.joint.clone());
        self.curve_selected.push(id);
    }

    /// Keys every given joint at the playhead on all six of its channels,
    /// keeping the interpolation and tangents a key there already had.
    pub(super) fn key_curves(&mut self, poses: &[(String, Frame)]) {
        let time = self.snap(self.time);
        self.edit_curves(|curves| {
            for (joint, frame) in poses {
                let curves = curves.joints.entry(joint.clone()).or_default();
                let near = glam::Vec3::from_array(
                    curves
                        .rotation
                        .each_ref()
                        .map(|c| c.eval(time).unwrap_or(0.)),
                );
                let angles = curves
                    .order
                    .unwrap(curves.order.decompose(frame.rotation), near);
                let values = [
                    frame.position.x,
                    frame.position.y,
                    frame.position.z,
                    angles.x,
                    angles.y,
                    angles.z,
                ];
                for channel in Channel::ALL {
                    let curve = curves.curve_mut(channel);
                    let value = values[channel.index()];
                    match find(curve, time) {
                        Some(i) => {
                            curve.set_value(i, value);
                        }
                        None => {
                            curve.insert(FloatKey::new(time, value, Interpolation::Linear));
                        }
                    }
                }
            }
        });
    }

    /// Adds a key at the playhead on one channel, where the curve already
    /// passes, so the motion does not change until the key is moved.
    pub(crate) fn insert_curve_key(&mut self, joint: &str, channel: Channel) {
        let time = self.snap(self.time);
        self.edit_curves(|curves| {
            let curve = curves
                .joints
                .entry(joint.to_owned())
                .or_default()
                .curve_mut(channel);
            if find(curve, time).is_none() {
                let value = curve.eval(time).unwrap_or(0.);
                curve.insert(FloatKey::new(time, value, Interpolation::Linear));
            }
        });
        self.curve_selected = vec![CurveKeyId {
            joint: joint.to_owned(),
            channel,
            time,
        }];
    }

    pub(crate) fn delete_curve_keys(&mut self) -> bool {
        if self.curve_selected.is_empty() {
            return false;
        }
        let picked = std::mem::take(&mut self.curve_selected);
        self.edit_curves(|curves| {
            for id in &picked {
                if let Some(j) = curves.joints.get_mut(&id.joint) {
                    let curve = j.curve_mut(id.channel);
                    if let Some(i) = find(curve, id.time) {
                        curve.remove(i);
                    }
                }
            }
            curves.joints.retain(|_, j| !j.is_empty());
        });
        true
    }

    /// Applies `change` to every selected key's curve and index.
    fn each_selected(&mut self, mut change: impl FnMut(&mut FloatCurve, usize)) {
        let picked = self.curve_selected.clone();
        self.edit_curves(|curves| {
            for id in &picked {
                if let Some(j) = curves.joints.get_mut(&id.joint) {
                    let curve = j.curve_mut(id.channel);
                    if let Some(i) = find(curve, id.time) {
                        change(curve, i);
                    }
                }
            }
        });
    }

    pub(crate) fn set_curve_value(&mut self, value: f32) {
        self.each_selected(|curve, i| {
            curve.set_value(i, value);
        });
    }

    pub(crate) fn set_curve_interpolation(&mut self, mode: Interpolation) {
        self.each_selected(|curve, i| {
            curve.set_interpolation(i, mode);
        });
    }

    /// Sets the selected keys' tangents, in value units per second; `None`
    /// leaves that side as it is.
    pub(crate) fn set_curve_tangents(&mut self, left: Option<f32>, right: Option<f32>) {
        self.each_selected(|curve, i| {
            let key = curve.keys[i];
            curve.set_tangents(i, left.or(key.left_tangent), right.or(key.right_tangent));
        });
    }

    /// Clears the selected keys' tangents back to automatic.
    pub(crate) fn auto_curve_tangents(&mut self) {
        self.each_selected(|curve, i| {
            curve.set_tangents(i, None, None);
        });
    }

    /// The interpolation every selected key shares, if they agree.
    pub(crate) fn curve_interpolation(&self) -> Option<Interpolation> {
        let mut found = self
            .curve_selected
            .iter()
            .filter_map(|id| self.curve_key(id))
            .map(|k| k.interpolation);
        let first = found.next()?;
        found.all(|m| m == first).then_some(first)
    }

    /// The editor's Generate Curve: on each channel with a selected key,
    /// bakes `style` from its earliest selected key to its latest one, or
    /// to the next key when only one is selected there.
    pub(crate) fn generate_curve(&mut self, style: Style, direction: Direction) -> bool {
        let mut picked: Vec<(String, Channel, Vec<f32>)> = Vec::new();
        for id in &self.curve_selected {
            match picked
                .iter_mut()
                .find(|s| s.0 == id.joint && s.1 == id.channel)
            {
                Some(s) => s.2.push(id.time),
                None => picked.push((id.joint.clone(), id.channel, vec![id.time])),
            }
        }
        let spans: Vec<(String, Channel, f32, Option<f32>)> = picked
            .into_iter()
            .map(|(joint, channel, times)| {
                let lo = times.iter().copied().fold(f32::INFINITY, f32::min);
                let hi = times.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                (joint, channel, lo, (times.len() > 1).then_some(hi))
            })
            .collect();
        if spans.is_empty() {
            return false;
        }
        let fps = self.bake_fps();
        let before = self.curves.clone();
        self.edit_curves(|curves| {
            for (joint, channel, from, to) in &spans {
                let Some(j) = curves.joints.get_mut(joint) else {
                    continue;
                };
                let curve = j.curve_mut(*channel);
                let Some(a) = find(curve, *from) else {
                    continue;
                };
                let b = match to {
                    Some(t) => find(curve, *t),
                    None => (a + 1 < curve.keys.len()).then_some(a + 1),
                };
                if let Some(b) = b {
                    curve.generate(a, b, style, direction, fps);
                }
            }
        });
        self.curves != before
    }

    pub(crate) fn begin_curve_drag(&mut self) {
        let keys: Vec<(CurveKeyId, FloatKey)> = self
            .curve_selected
            .iter()
            .filter_map(|id| self.curve_key(id).map(|k| (id.clone(), k)))
            .collect();
        if !keys.is_empty() {
            self.curve_drag = Some(CurveDrag {
                origin: self.snapshot(),
                keys,
            });
        }
    }

    /// Moves the dragged keys `dt` seconds (snapped to frames, never before
    /// 0) and `dvalue` in each channel's own units: studs, or radians for a
    /// rotation channel (`dangle`).
    pub(crate) fn curve_drag_by(&mut self, dt: f32, dvalue: f32, dangle: f32) {
        let Some(drag) = &self.curve_drag else { return };
        let fps = self.bake_fps();
        let earliest = drag
            .keys
            .iter()
            .map(|k| k.1.time)
            .fold(f32::INFINITY, f32::min);
        let shift = ((dt * fps).round() / fps).max(-earliest);
        let Some(mut curves) = drag.origin.curves.clone() else {
            return;
        };
        let mut moved = Vec::new();
        for (id, _) in &drag.keys {
            if let Some(j) = curves.joints.get_mut(&id.joint) {
                let curve = j.curve_mut(id.channel);
                if let Some(i) = find(curve, id.time) {
                    curve.remove(i);
                }
            }
        }
        for (id, key) in &drag.keys {
            let curve = curves
                .joints
                .entry(id.joint.clone())
                .or_default()
                .curve_mut(id.channel);
            let mut key = *key;
            key.time = (key.time + shift).max(0.);
            key.value += if id.channel.is_rotation() {
                dangle
            } else {
                dvalue
            };
            curve.insert(key);
            moved.push(CurveKeyId {
                time: key.time,
                ..id.clone()
            });
        }
        self.curves = Some(curves);
        self.bake();
        self.curve_selected = moved;
    }

    /// Points one dragged key's tangent at a slope (value units per
    /// second), on its left or right side. A Cubic key's handles are set
    /// apart; any other key has only the left one, which mirrors.
    pub(crate) fn drag_tangent(&mut self, left: bool, slope: f32) {
        let Some(drag) = &self.curve_drag else { return };
        let Some((id, _)) = drag.keys.first().cloned() else {
            return;
        };
        let Some(mut curves) = drag.origin.curves.clone() else {
            return;
        };
        if let Some(curve) = curves
            .joints
            .get_mut(&id.joint)
            .map(|j| j.curve_mut(id.channel))
        {
            if let Some(i) = find(curve, id.time) {
                let key = curve.keys[i];
                let (l, r) = if left || key.interpolation != Interpolation::Cubic {
                    (Some(slope), key.right_tangent)
                } else {
                    (key.left_tangent.or(Some(key.tangents().0)), Some(slope))
                };
                curve.set_tangents(i, l, r);
            }
        }
        self.curves = Some(curves);
        self.bake();
    }

    /// Moves the one selected key to `time` (snapped), replacing any key
    /// already there on that channel.
    pub(crate) fn set_curve_time(&mut self, time: f32) {
        let [id] = self.curve_selected.as_slice() else {
            return;
        };
        let id = id.clone();
        let to = self.snap(time);
        self.edit_curves(|curves| {
            if let Some(curve) = curves
                .joints
                .get_mut(&id.joint)
                .map(|j| j.curve_mut(id.channel))
            {
                if let Some(i) = find(curve, id.time) {
                    let mut key = curve.remove(i).expect("found");
                    if let Some(j) = find(curve, to) {
                        curve.remove(j);
                    }
                    key.time = to;
                    curve.insert(key);
                }
            }
        });
        self.curve_selected = vec![CurveKeyId { time: to, ..id }];
    }

    pub(crate) fn end_curve_drag(&mut self) {
        if let Some(drag) = self.curve_drag.take() {
            self.commit(drag.origin);
        }
    }

    /// What the editor plays at `time`: the curves when there are some,
    /// else the clip.
    pub(crate) fn sample_at(&self, time: f32) -> std::collections::BTreeMap<String, Frame> {
        match &self.curves {
            Some(curves) => curves.sample(time),
            None => rbx_animation::sample::sample(&self.clip, time)
                .into_iter()
                .map(|(j, s)| (j, Frame::IDENTITY.lerp(&s.transform, s.weight.min(1.0))))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
