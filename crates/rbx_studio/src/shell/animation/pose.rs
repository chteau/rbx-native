//! Puts the bound rig in the pose the clip (or the graph) has at the
//! playhead, every frame, and back in its rest pose on the way out.
//!
//! As Roblox's Animator does, the pose is written to each joint's
//! `Transform` (a `Motor6D`'s or an `AnimationConstraint`'s), never to the
//! parts: their `CFrame`s stay at rest in the document, and the 3D view's
//! joint solver (`rbx_viewer::JointSolver`) lays them out from the joints.
//! What this side reads a part's place from (the selection's boxes and
//! handles, picking) sees the pose through [`Shell::pose_in`].

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use gpui_kit::*;
use rbx_animation::{Frame, Rig};
use rbx_dom::{CFrameData, Change, Ref, Variant, WeakDom};

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

fn cframe_of(dom: &WeakDom, part: Ref) -> Option<CFrameData> {
    match dom.get(part)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(*c),
        _ => None,
    }
}

/// Writes each of `rig`'s joints' `Transform` from `transforms` (keyed by
/// the name of the part the joint drives; identity when absent), first
/// keeping in `originals` what each joint held before the editor touched it.
/// Whether anything was written.
pub(super) fn write_transforms(
    dom: &mut WeakDom,
    rig: &Rig,
    transforms: &BTreeMap<String, Frame>,
    originals: &mut HashMap<Ref, Option<Variant>>,
) -> bool {
    let mut wrote = false;
    for joint in &rig.joints {
        let frame = transforms
            .get(rig.part_name(joint.part1))
            .copied()
            .unwrap_or_default();
        let value = Variant::CFrame(frame.to_data());
        let Some(instance) = dom.get(joint.instance) else {
            continue;
        };
        let now = instance.properties().get("Transform");
        if now == Some(&value) {
            continue;
        }
        originals
            .entry(joint.instance)
            .or_insert_with(|| now.cloned());
        wrote |= dom.set_property(joint.instance, "Transform", value).is_ok();
    }
    wrote
}

/// Puts back every `Transform` [`write_transforms`] changed as it found it
/// (taken away again where there was none), so a document saved after
/// playback holds exactly what it held before. Whether anything was written.
pub(super) fn restore_transforms(
    dom: &mut WeakDom,
    originals: &mut HashMap<Ref, Option<Variant>>,
) -> bool {
    let mut wrote = false;
    for (joint, original) in originals.drain() {
        if dom.get(joint).is_none() {
            continue;
        }
        wrote |= match original {
            Some(v) => dom.set_property(joint, "Transform", v).is_ok(),
            None => dom.remove_property(joint, "Transform").is_ok(),
        };
    }
    wrote
}

/// Two rotations closer than this (`1 - |dot|` of their quaternions, about
/// 0.1° apart) are the same: a Move drag's solve only adds rounding.
const SAME_TURN: f32 = 1e-6;

/// The front-most part under a click, when it is one of the rig's posable
/// parts (`rigged`): what a plain click selects while posing, so the drag
/// that follows lands on that part's joint. Anything else in front — a
/// wall, another model — leaves the ordinary resolution to run.
///
/// The rig's `root` (an R15's `HumanoidRootPart`, invisible and wrapped
/// round the torso) is passed over when a limb of the rig stands behind it,
/// as Studio's Animation Editor lets a click on the torso reach the torso;
/// a click that finds nothing but the root still takes it.
pub(super) fn joint_part(
    hits: &[Ref],
    root: Option<Ref>,
    rigged: impl Fn(&Ref) -> bool,
) -> Option<Ref> {
    let (&first, rest) = hits.split_first()?;
    if Some(first) == root {
        if let Some(&behind) = rest.first().filter(|part| rigged(part)) {
            return Some(behind);
        }
    }
    rigged(&first).then_some(first)
}

/// The key a viewport drag leaves on a joint whose `Transform` was `now`,
/// given `solved`, the `Transform` that puts the part where the drag left it
/// (see `rbx_animation::Joint::solve`).
///
/// A drag that turned nothing is a Move: its offset in joint space is the
/// key's translation, as Studio's Animation Editor keys a moved joint (a
/// pose's `CFrame` carries both halves). A drag that turned the part keeps
/// the translation the joint had: the Rotate tool turns a limb about its
/// joint's pivot, and a rotation about any other point (a part's centre)
/// still pivots at the shoulder rather than sliding the limb off it.
pub(super) fn drag_key(now: Frame, solved: Frame) -> Frame {
    let turned = 1.0 - now.rotation.dot(solved.rotation).abs() > SAME_TURN;
    if turned {
        Frame::new(now.position, solved.rotation)
    } else {
        Frame::new(solved.position, now.rotation)
    }
}

/// What [`Shell::pose_in`] swapped into the document, for [`Shell::pose_out`]
/// to put back.
#[must_use]
pub(in crate::shell) struct PoseShown(Vec<(Ref, Option<Variant>, CFrameData)>);

impl Shell {
    /// Every joint's `Transform` as the document should show it now.
    pub(super) fn current_transforms(&self) -> BTreeMap<String, Frame> {
        if self.anim.mode == Mode::Graph {
            if let Some(pose) = &self.anim.graph.pose {
                return pose.clone();
            }
        }
        self.anim.editor.sample_at(self.anim.editor.time)
    }

    /// Writes each joint's `Transform` for the current pose (the parts stay
    /// at rest; the 3D view lays them out), and keeps where that puts each
    /// part for the skeleton, IK and [`Shell::pose_in`].
    pub(in crate::shell::animation) fn pose_rig(&mut self, cx: &mut Context<Self>) {
        self.adopt_rest();
        let changes = self.write_pose();
        let (selected, world) = match &self.anim.bound {
            Some(bound) => (
                self.selected_all()
                    .iter()
                    .any(|s| bound.posed.contains_key(s)),
                bound.posed.clone(),
            ),
            None => return,
        };
        if !changes.is_empty() {
            self.reflect_changes(&changes, cx);
            // A Transform names the joint, not the part it moves: a selected
            // limb's box and handles are re-read here.
            if selected {
                self.reshow_selection(cx);
            }
        }
        let world = world
            .into_iter()
            .map(|(p, c)| (p, Frame::from_data(&c)))
            .collect();
        self.sync_skeleton(&world, cx);
    }

    /// [`Shell::pose_rig`]'s writes, without reflecting them: the log.
    fn write_pose(&mut self) -> Vec<Change> {
        let transforms = self.current_transforms();
        let Some(bound) = &mut self.anim.bound else {
            return Vec::new();
        };
        if let Some(root) = bound.rig.root.and_then(|r| bound.rest.get(&r)) {
            bound.root = Frame::from_data(root);
        }
        let world = bound
            .rig
            .layout(bound.root, &|j| transforms.get(j).copied());
        bound.posed = world.iter().map(|(&p, f)| (p, f.to_data())).collect();
        if write_transforms(
            &mut self.dom,
            &bound.rig,
            &transforms,
            &mut bound.transforms,
        ) {
            self.dom.take_changes()
        } else {
            Vec::new()
        }
    }

    /// A rig part's `CFrame` that something other than a viewport drag
    /// changed (an undo, the Properties panel, a script) is its new rest.
    fn adopt_rest(&mut self) {
        let Some(bound) = &mut self.anim.bound else {
            return;
        };
        for (&part, rest) in bound.rest.iter_mut() {
            if let Some(now) = cframe_of(&self.dom, part) {
                if !same(&now, rest) {
                    *rest = now;
                }
            }
        }
    }

    /// Swaps each posed rig part's `CFrame` in the document for the one it
    /// is drawn at, untracked, so a read of where things stand (a hit test,
    /// the selection's boxes and handles) finds the pose the 3D view shows.
    /// Always paired with [`Shell::pose_out`], with nothing between that
    /// writes the document or snapshots it for the view.
    pub(in crate::shell) fn pose_in(&mut self) -> PoseShown {
        let mut swapped = Vec::new();
        let Some(bound) = &self.anim.bound else {
            return PoseShown(swapped);
        };
        for (&part, posed) in &bound.posed {
            if bound.rest.get(&part).is_some_and(|r| same(r, posed)) {
                continue;
            }
            if let Some(instance) = self.dom.get_mut(part) {
                let old = instance
                    .properties_mut()
                    .insert("CFrame".to_owned(), Variant::CFrame(*posed));
                swapped.push((part, old, *posed));
            }
        }
        PoseShown(swapped)
    }

    /// Puts back what [`Shell::pose_in`] swapped — except a `CFrame` written
    /// in between (a drag step on an already posed limb), which is left for
    /// [`Shell::settle_pose_edits`] to key. Restoring it too would throw away
    /// every drag step after the first on a limb that is no longer at rest.
    pub(in crate::shell) fn pose_out(&mut self, shown: PoseShown) {
        for (part, old, posed) in shown.0 {
            if let Some(instance) = self.dom.get_mut(part) {
                let written = !matches!(
                    instance.properties().get("CFrame"),
                    Some(Variant::CFrame(now)) if same(now, &posed)
                );
                if written {
                    continue;
                }
                match old {
                    Some(v) => instance.properties_mut().insert("CFrame".to_owned(), v),
                    None => instance.properties_mut().remove("CFrame"),
                };
            }
        }
    }

    /// A viewport drag on a posed rig moves parts where they are drawn; this
    /// turns that into a pose and puts the parts back at rest, before the
    /// drag's log goes anywhere. A part moved or turned gets a key on its
    /// joint at the playhead (see [`drag_key`]); with IK on, a moved part
    /// drags its chain instead. A moved root part moves the
    /// whole rig, rest and all. Returns the log of what it wrote, for the
    /// caller to add to the drag's own.
    pub(in crate::shell) fn settle_pose_edits(&mut self, changes: &[Change]) -> Vec<Change> {
        let Some(bound) = &self.anim.bound else {
            return Vec::new();
        };
        let touched = changes.iter().any(|c| {
            matches!(c, Change::Property { referent, name }
                if name == "CFrame" && bound.rest.contains_key(referent))
        });
        if !touched {
            return Vec::new();
        }
        let editing = !self.anim.editor.playing && self.anim.mode != Mode::Graph;
        let mut edited: Vec<(Ref, CFrameData)> = Vec::new();
        for (&part, rest) in &bound.rest {
            if let Some(now) = cframe_of(&self.dom, part) {
                if !same(&now, rest) {
                    edited.push((part, now));
                }
            }
        }
        let root = bound.rig.root;
        let moved_root = edited.iter().find(|(p, _)| Some(*p) == root).copied();

        let mut keys = Vec::new();
        let mut dragged: Option<(Ref, glam::Vec3)> = None;
        if let Some((root, now)) = moved_root {
            // The whole rig moved: its rest goes with it.
            let rest = bound.rest[&root];
            let delta = Frame::from_data(&now).mul(&Frame::from_data(&rest).inverse());
            let Some(bound) = &mut self.anim.bound else {
                return Vec::new();
            };
            for rest in bound.rest.values_mut() {
                *rest = delta.mul(&Frame::from_data(rest)).to_data();
            }
        } else if editing {
            for &(part, now) in &edited {
                let Some(joint) = bound.rig.joint_driving(part) else {
                    continue;
                };
                let Some(part0) = bound.posed.get(&joint.part0).map(Frame::from_data) else {
                    continue;
                };
                let now = Frame::from_data(&now);
                let posed = bound.posed.get(&part).map_or(now, Frame::from_data);
                // With IK on, a part moved (not just turned) is an end
                // effector dragged to a new place: its chain is solved to
                // reach it.
                let moved = now.position.distance(posed.position) > 1e-3;
                if self.anim.ik.enabled && moved {
                    dragged = Some((part, now.position));
                    continue;
                }
                let solved = joint.solve(&part0, &now);
                keys.push((bound.rig.part_name(part).to_owned(), solved));
            }
        }
        if let Some((part, target)) = dragged {
            if let Some(bound) = &self.anim.bound {
                let current = self.current_transforms();
                let solved = rbx_animation::ik::solve(
                    &bound.rig,
                    bound.root,
                    &current,
                    part,
                    glam::Vec3::ZERO,
                    target,
                    self.anim.ik.mode,
                    &self.anim.ik.pinned,
                );
                let changed: Vec<(String, Frame)> = solved
                    .into_iter()
                    .filter(|(j, f)| current.get(j) != Some(f))
                    .collect();
                let name = bound.rig.part_name(part).to_owned();
                if !changed.is_empty() {
                    self.anim.editor.key_all(&changed);
                }
                self.anim.editor.joint = Some(name);
            }
        } else {
            let current = self.current_transforms();
            for (name, solved) in keys {
                let now = current.get(&name).copied().unwrap_or_default();
                self.anim.editor.key(&name, drag_key(now, solved));
                self.anim.editor.joint = Some(name);
            }
        }
        // Every part back at rest; the pose is the joints' to carry.
        let Some(bound) = &self.anim.bound else {
            return Vec::new();
        };
        let rest: Vec<(Ref, CFrameData)> = bound.rest.iter().map(|(&p, &c)| (p, c)).collect();
        for (part, rest) in rest {
            if cframe_of(&self.dom, part).is_some_and(|now| now != rest) {
                let _ = self.dom.set_property(part, "CFrame", Variant::CFrame(rest));
            }
        }
        let mut log = self.dom.take_changes();
        log.extend(self.write_pose());
        log
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

    /// Puts every joint's `Transform` back as the editor found it, and
    /// every part its own pivot: the document as it was before posing.
    pub(in crate::shell) fn unpose_rig(&mut self, cx: &mut Context<Self>) {
        let Some(bound) = &mut self.anim.bound else {
            return;
        };
        for (part, original) in bound.pivots.drain() {
            let _ = match original {
                Some(v) => self.dom.set_property(part, "PivotOffset", v).map(|_| ()),
                None => self.dom.remove_property(part, "PivotOffset").map(|_| ()),
            };
        }
        restore_transforms(&mut self.dom, &mut bound.transforms);
        bound.posed.clear();
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
    }

    /// Takes the editor's own writes (each joint's `Transform`, the joint
    /// pivots) out of `copy`, a clone of the document, as
    /// [`Shell::unpose_rig`] does to the document itself: what the recovery
    /// copy holds is the place as it would save.
    pub(in crate::shell) fn unpose_copy(&self, copy: &mut WeakDom) {
        let Some(bound) = &self.anim.bound else {
            return;
        };
        for (&part, original) in &bound.pivots {
            let _ = match original {
                Some(v) => copy
                    .set_property(part, "PivotOffset", v.clone())
                    .map(|_| ()),
                None => copy.remove_property(part, "PivotOffset").map(|_| ()),
            };
        }
        restore_transforms(copy, &mut bound.transforms.clone());
        copy.take_changes();
    }

    /// One frame of the document: advances playback, poses the rig, and
    /// asks for the next frame while anything is moving.
    /// What a click on the posed rig takes while posing: the rig part
    /// nearest under the cursor — the part whose joint a drag keys — rather
    /// than the whole rig `Model` a click elsewhere would take. `Alt` keeps
    /// its cycling (`None` here, so the ordinary resolution runs), and so
    /// does anything that isn't one of the rig's parts.
    pub(in crate::shell) fn joint_pick(&self, hits: &[Ref], cycling: bool) -> Option<Ref> {
        if !self.transform.posing || cycling {
            return None;
        }
        let bound = self.anim.bound.as_ref()?;
        joint_part(hits, bound.rig.root, |part| bound.rest.contains_key(part))
    }

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
        let posing = self.anim.bound.is_some();
        self.set_posing(posing, cx);
        self.pose_rig(cx);
        self.sync_joint_pivot(cx);
        self.keep_draft(false);
        if moving {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
    }
}
