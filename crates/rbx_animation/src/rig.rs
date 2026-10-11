//! Finds a rig's animatable joints and lays its parts out through them.
//!
//! Two classes animate a rig. A `Motor6D` joins `Part0` to `Part1` through
//! `C0`/`C1`. An `AnimationConstraint` (what the Avatar Joint Upgrade builds
//! instead) joins two `Attachment`s, which carry the same two offsets as their
//! own `CFrame`s, relative to their parent parts. Both are reduced here to one
//! [`Joint`], so playback, the editor and the gizmos never have to care which
//! one a rig was built with:
//!
//! `Part1.CFrame = Part0.CFrame * C0 * Transform * C1:Inverse()`

use std::collections::{HashMap, HashSet};

use rbx_dom::{Ref, Variant, WeakDom};

use crate::frame::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JointKind {
    Motor6D,
    AnimationConstraint,
}

impl JointKind {
    pub fn class(self) -> &'static str {
        match self {
            JointKind::Motor6D => "Motor6D",
            JointKind::AnimationConstraint => "AnimationConstraint",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// The `Motor6D` or `AnimationConstraint` itself, whose `Transform` an
    /// animation drives.
    pub instance: Ref,
    pub kind: JointKind,
    pub name: String,
    pub part0: Ref,
    pub part1: Ref,
    pub c0: Frame,
    pub c1: Frame,
}

impl Joint {
    /// Where `Part1` sits for a given `Transform`, with `Part0` at `part0`.
    pub fn place(&self, part0: &Frame, transform: &Frame) -> Frame {
        part0
            .mul(&self.c0)
            .mul(transform)
            .mul(&self.c1.inverse())
    }

    /// The `Transform` that puts `Part1` at `part1`: [`Joint::place`] solved
    /// for its middle term, which is how a limb dragged in the viewport
    /// becomes a pose.
    pub fn solve(&self, part0: &Frame, part1: &Frame) -> Frame {
        part0
            .mul(&self.c0)
            .inverse()
            .mul(part1)
            .mul(&self.c1)
    }

    /// The joint's pivot in world space: the frame `Transform` turns about.
    pub fn pivot(&self, part0: &Frame) -> Frame {
        part0.mul(&self.c0)
    }
}

/// A rig's joints as a tree hanging off one root part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig {
    pub model: Option<Ref>,
    pub root: Option<Ref>,
    /// Ordered parents-first, so a single pass over it can place every part.
    pub joints: Vec<Joint>,
    pub names: HashMap<Ref, String>,
}

pub(crate) fn cframe(dom: &WeakDom, of: Ref, property: &str) -> Option<Frame> {
    match dom.get(of)?.properties().get(property)? {
        Variant::CFrame(data) => Some(Frame::from_data(data)),
        Variant::OptionalCFrame(Some(data)) => Some(Frame::from_data(data)),
        _ => None,
    }
}

pub(crate) fn reference(dom: &WeakDom, of: Ref, property: &str) -> Option<Ref> {
    match dom.get(of)?.properties().get(property)? {
        Variant::Ref(target) if dom.get(*target).is_some() => Some(*target),
        _ => None,
    }
}

fn descendants(dom: &WeakDom, of: Ref, out: &mut Vec<Ref>) {
    let Some(instance) = dom.get(of) else {
        return;
    };
    for &child in instance.children() {
        out.push(child);
        descendants(dom, child, out);
    }
}

fn joint(dom: &WeakDom, of: Ref) -> Option<Joint> {
    let instance = dom.get(of)?;
    let (kind, part0, part1, c0, c1) = match instance.class() {
        "Motor6D" => (
            JointKind::Motor6D,
            reference(dom, of, "Part0")?,
            reference(dom, of, "Part1")?,
            cframe(dom, of, "C0").unwrap_or_default(),
            cframe(dom, of, "C1").unwrap_or_default(),
        ),
        "AnimationConstraint" => {
            let a0 = reference(dom, of, "Attachment0")?;
            let a1 = reference(dom, of, "Attachment1")?;
            (
                JointKind::AnimationConstraint,
                dom.parent(a0)?,
                dom.parent(a1)?,
                cframe(dom, a0, "CFrame").unwrap_or_default(),
                cframe(dom, a1, "CFrame").unwrap_or_default(),
            )
        }
        _ => return None,
    };
    Some(Joint {
        instance: of,
        kind,
        name: instance.name().to_owned(),
        part0,
        part1,
        c0,
        c1,
    })
}

impl Rig {
    /// Reads the rig under `model`. The root is the part no joint drives:
    /// `HumanoidRootPart` on any avatar, otherwise whichever part sits at the
    /// top of the joint tree (falling back to `PrimaryPart`).
    pub fn read(dom: &WeakDom, model: Ref) -> Rig {
        let mut all = Vec::new();
        descendants(dom, model, &mut all);
        let found: Vec<Joint> = all.iter().filter_map(|&r| joint(dom, r)).collect();

        let driven: HashSet<Ref> = found.iter().map(|j| j.part1).collect();
        let named_root = all.iter().copied().find(|&r| {
            dom.get(r)
                .is_some_and(|i| i.name() == "HumanoidRootPart" && !driven.contains(&r))
        });
        let root = named_root
            .or_else(|| {
                found
                    .iter()
                    .map(|j| j.part0)
                    .find(|part| !driven.contains(part))
            })
            .or_else(|| reference(dom, model, "PrimaryPart"));

        let mut joints = Vec::with_capacity(found.len());
        let mut placed: HashSet<Ref> = root.into_iter().collect();
        let mut pending = found;
        // Breadth-first from the root; a joint whose Part0 never gets placed
        // (a second, disconnected assembly) is dropped rather than guessed at.
        loop {
            let (ready, rest): (Vec<Joint>, Vec<Joint>) = pending
                .into_iter()
                .partition(|j| placed.contains(&j.part0) && !placed.contains(&j.part1));
            if ready.is_empty() {
                break;
            }
            for j in ready {
                if placed.insert(j.part1) {
                    joints.push(j);
                }
            }
            pending = rest;
        }

        let mut names = HashMap::new();
        for r in root.iter().chain(joints.iter().map(|j| &j.part1)) {
            if let Some(i) = dom.get(*r) {
                names.insert(*r, i.name().to_owned());
            }
        }
        Rig {
            model: Some(model),
            root,
            joints,
            names,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }

    pub fn joint_for(&self, part1_name: &str) -> Option<&Joint> {
        self.joints
            .iter()
            .find(|j| self.names.get(&j.part1).map(String::as_str) == Some(part1_name))
    }

    pub fn joint_driving(&self, part: Ref) -> Option<&Joint> {
        self.joints.iter().find(|j| j.part1 == part)
    }

    pub fn part_name(&self, part: Ref) -> &str {
        self.names.get(&part).map(String::as_str).unwrap_or("")
    }

    /// The pose tree's parent of each part, by name: the hierarchy a
    /// `KeyframeSequence` nests its `Pose`s in.
    pub fn parent_name(&self, part1_name: &str) -> Option<&str> {
        self.joint_for(part1_name).map(|j| self.part_name(j.part0))
    }

    /// Every part's world frame with the root at `root` and each joint's
    /// `Transform` looked up by its `Part1` name (identity when absent).
    pub fn layout(
        &self,
        root: Frame,
        transforms: &dyn Fn(&str) -> Option<Frame>,
    ) -> HashMap<Ref, Frame> {
        let mut world = HashMap::with_capacity(self.joints.len() + 1);
        let Some(root_ref) = self.root else {
            return world;
        };
        world.insert(root_ref, root);
        for j in &self.joints {
            let Some(&part0) = world.get(&j.part0) else {
                continue;
            };
            let transform = transforms(self.part_name(j.part1)).unwrap_or_default();
            world.insert(j.part1, j.place(&part0, &transform));
        }
        world
    }
}

#[cfg(test)]
mod tests;
