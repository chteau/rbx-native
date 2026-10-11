//! Puts the bound rig in the pose the clip (or the graph) has at the
//! playhead, every frame, and back in its rest pose on the way out.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use gpui_kit::*;
use rbx_animation::{sample, Frame};
use rbx_dom::{CFrameData, Ref, Variant};

use super::{Mode, Shell};

/// Positions within this many studs, and rotation entries within this much,
/// are the same `CFrame`: well under what a drag could move.
const SAME: f32 = 1e-4;

fn same(a: &CFrameData, b: &CFrameData) -> bool {
    let p = |v: &rbx_dom::Vector3Data| [v.x, v.y, v.z];
    p(&a.position)
        .iter()
        .zip(p(&b.position))
        .all(|(x, y)| (x - y).abs() < SAME)
        && a.rotation
            .iter()
            .zip(b.rotation)
            .all(|(x, y)| (x - y).abs() < SAME)
}

fn cframe_of(dom: &rbx_dom::WeakDom, part: Ref) -> Option<CFrameData> {
    match dom.get(part)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(*c),
        _ => None,
    }
}

impl Shell {
    /// Every joint's `Transform` as the document should show it now.
    pub(super) fn current_transforms(&self) -> BTreeMap<String, Frame> {
        if self.anim.mode == Mode::Graph {
            if let Some(pose) = &self.anim.graph.pose {
                return pose.clone();
            }
        }
        let clip = &self.anim.editor.clip;
        sample::sample(clip, self.anim.editor.time)
            .into_iter()
            .map(|(j, s)| (j, Frame::IDENTITY.lerp(&s.transform, s.weight.min(1.0))))
            .collect()
    }

    /// Writes each rig part's `CFrame` for the current pose, and first reads
    /// back any part moved by something else (a viewport drag) as a key.
    pub(in crate::shell::animation) fn pose_rig(&mut self, cx: &mut Context<Self>) {
        self.read_back_edits();
        let transforms = self.current_transforms();
        let Some(bound) = &mut self.anim.bound else {
            return;
        };
        let world = bound
            .rig
            .layout(bound.root, &|j| transforms.get(j).copied());
        let mut wrote = false;
        for (part, frame) in world {
            let data = frame.to_data();
            if cframe_of(&self.dom, part).is_some_and(|c| same(&c, &data)) {
                bound.posed.insert(part, data);
                continue;
            }
            if self
                .dom
                .set_property(part, "CFrame", Variant::CFrame(data))
                .is_ok()
            {
                bound.posed.insert(part, data);
                wrote = true;
            }
        }
        if wrote {
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
        }
    }

    /// A rig part whose `CFrame` is not what the editor last wrote was
    /// moved by hand. Its joint gets a key at the playhead turning it that
    /// way, about the joint (the drag's own shift is dropped, so a limb
    /// rotated about its centre still pivots at the shoulder). A moved root
    /// part moves the whole rig.
    fn read_back_edits(&mut self) {
        if self.anim.editor.playing || self.anim.mode == Mode::Graph {
            return;
        }
        let Some(bound) = &self.anim.bound else {
            return;
        };
        let mut root = None;
        let mut keys = Vec::new();
        for (&part, posed) in &bound.posed {
            let Some(now) = cframe_of(&self.dom, part) else {
                continue;
            };
            if same(&now, posed) {
                continue;
            }
            if Some(part) == bound.rig.root {
                root = Some(Frame::from_data(&now));
                continue;
            }
            let Some(joint) = bound.rig.joint_driving(part) else {
                continue;
            };
            let Some(part0) = bound.posed.get(&joint.part0).map(Frame::from_data) else {
                continue;
            };
            let solved = joint.solve(&part0, &Frame::from_data(&now));
            let name = bound.rig.part_name(part).to_owned();
            keys.push((name, solved));
        }
        if let Some(root) = root {
            if let Some(b) = &mut self.anim.bound {
                b.root = root;
            }
        }
        let current = self.current_transforms();
        for (name, solved) in keys {
            let kept = current.get(&name).map_or(glam::Vec3::ZERO, |f| f.position);
            self.anim
                .editor
                .key(&name, Frame::new(kept, solved.rotation));
            self.anim.editor.joint = Some(name);
        }
    }

    /// The joint gizmo: the selected rig part's `PivotOffset` is moved onto
    /// its joint (its `C1`), so the Rotate tool's rings stand on the
    /// shoulder or hip and turn the limb about it. The same holds for a
    /// `Motor6D` or an `AnimationConstraint`, since both are a `Joint` here.
    /// Every other part the editor moved gets its own pivot back.
    fn sync_joint_pivot(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_all().first().copied();
        let Some(bound) = &mut self.anim.bound else {
            return;
        };
        let wanted = selected.and_then(|s| bound.rig.joint_driving(s).map(|j| (s, j.c1)));
        let mut wrote = false;
        let stale: Vec<Ref> = bound
            .pivots
            .keys()
            .copied()
            .filter(|p| Some(*p) != wanted.map(|w| w.0))
            .collect();
        for part in stale {
            if let Some(original) = bound.pivots.remove(&part) {
                wrote |= match original {
                    Some(v) => self.dom.set_property(part, "PivotOffset", v).is_ok(),
                    None => self.dom.remove_property(part, "PivotOffset").is_ok(),
                };
            }
        }
        if let Some((part, c1)) = wanted {
            let pivot = Variant::CFrame(c1.to_data());
            let now = self
                .dom
                .get(part)
                .and_then(|i| i.properties().get("PivotOffset").cloned());
            if now.as_ref() != Some(&pivot) {
                bound.pivots.entry(part).or_insert(now);
                wrote |= self.dom.set_property(part, "PivotOffset", pivot).is_ok();
            }
        }
        if wrote {
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
        }
    }

    /// Puts every rig part back where it was before the editor posed it,
    /// with its own pivot.
    pub(in crate::shell) fn unpose_rig(&mut self, cx: &mut Context<Self>) {
        if let Some(bound) = &mut self.anim.bound {
            let originals: Vec<(Ref, Option<Variant>)> = bound.pivots.drain().collect();
            for (part, original) in originals {
                let _ = match original {
                    Some(v) => self.dom.set_property(part, "PivotOffset", v).map(|_| ()),
                    None => self.dom.remove_property(part, "PivotOffset").map(|_| ()),
                };
            }
        }
        let Some(bound) = &mut self.anim.bound else {
            return;
        };
        if bound.posed.is_empty() {
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
            return;
        }
        // The root may have been moved on purpose: the rest pose follows it.
        let rest: HashMap<Ref, CFrameData> = match bound.rig.root {
            Some(root) => {
                let at = bound.root;
                bound
                    .rig
                    .layout(at, &|_| None)
                    .into_iter()
                    .map(|(p, f)| (p, f.to_data()))
                    .chain(std::iter::once((root, at.to_data())))
                    .collect()
            }
            None => bound.rest.clone(),
        };
        bound.posed.clear();
        let mut wrote = false;
        for (part, data) in rest {
            if cframe_of(&self.dom, part).is_some_and(|c| !same(&c, &data)) {
                wrote |= self
                    .dom
                    .set_property(part, "CFrame", Variant::CFrame(data))
                    .is_ok();
            }
        }
        if wrote {
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
        }
    }

    /// One frame of the document: advances playback, poses the rig, and
    /// asks for the next frame while anything is moving.
    pub(super) fn animation_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .anim
            .bound
            .as_ref()
            .is_some_and(|b| self.dom.get(b.model).is_none())
        {
            self.anim.bound = None;
        }
        let now = Instant::now();
        let dt = self
            .anim
            .last_frame
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        let moving = self.anim.editor.playing || self.anim.graph.playing;
        self.anim.last_frame = moving.then_some(now);
        if self.anim.editor.playing {
            self.anim.editor.tick(dt);
        }
        if self.anim.graph.playing {
            self.anim_step_graph(dt);
        }
        self.pose_rig(cx);
        self.sync_joint_pivot(cx);
        self.keep_draft(false);
        if moving {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
    }
}
