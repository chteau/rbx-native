//! A transform map being edited, and turning one joint by a world-space
//! rotation, which is the one operation every solver here reduces to.

use std::collections::{BTreeMap, HashMap};

use glam::{Quat, Vec3};
use rbx_dom::Ref;

use crate::frame::Frame;
use crate::rig::Rig;

/// Below this a bone or offset has no usable direction.
pub(super) const EPSILON: f32 = 1e-6;

/// The dragged part and the grabbed point on it, in its own space.
pub(super) type Effector = (Ref, Vec3);

pub(super) struct Pose<'a> {
    rig: &'a Rig,
    root: Frame,
    transforms: BTreeMap<String, Frame>,
}

pub(super) type World = HashMap<Ref, Frame>;

fn at(world: &World, part: Ref) -> Frame {
    world.get(&part).copied().unwrap_or_default()
}

impl<'a> Pose<'a> {
    pub fn new(rig: &'a Rig, root: Frame, transforms: BTreeMap<String, Frame>) -> Self {
        Pose {
            rig,
            root,
            transforms,
        }
    }

    pub fn into_transforms(self) -> BTreeMap<String, Frame> {
        self.transforms
    }

    pub fn world(&self) -> World {
        self.rig
            .layout(self.root, &|name| self.transforms.get(name).copied())
    }

    pub fn part1(&self, joint: usize) -> Ref {
        self.rig.joints[joint].part1
    }

    fn transform(&self, joint: usize) -> Frame {
        let name = self.rig.part_name(self.part1(joint));
        self.transforms.get(name).copied().unwrap_or_default()
    }

    /// The frame a joint's `Transform` rotation turns about: past `C0` and the
    /// `Transform`'s own translation, which a solve never changes.
    fn hinge(&self, world: &World, joint: usize) -> Frame {
        let j = &self.rig.joints[joint];
        at(world, j.part0)
            .mul(&j.c0)
            .mul(&Frame::at(self.transform(joint).position))
    }

    pub fn pivot(&self, world: &World, joint: usize) -> Vec3 {
        self.hinge(world, joint).position
    }

    pub fn effector(&self, world: &World, (part, local): Effector) -> Vec3 {
        at(world, part).transform_point(local)
    }

    /// Turns `joint`'s Part1 (and everything below it) by `delta` in world
    /// space about the joint's pivot. `world` must be the current layout, as
    /// the conversion depends on where Part0 sits now.
    pub fn turn(&mut self, world: &World, joint: usize, delta: Quat) {
        let hinge = self.hinge(world, joint).rotation;
        let old = self.transform(joint);
        // Part1's world rotation is hinge * R * C1⁻¹; prefixing it with
        // `delta` means R' = hinge⁻¹ * delta * hinge * R.
        let rotation = (hinge.conjugate() * delta * hinge * old.rotation).normalize();
        let name = self.rig.part_name(self.part1(joint)).to_owned();
        self.transforms
            .insert(name, Frame::new(old.position, rotation));
    }
}

/// Turns a single joint so the line from its pivot through the effector
/// passes through `target`: the closest a lone joint gets without stretching.
pub(super) fn aim(pose: &mut Pose, joint: usize, effector: Effector, target: Vec3) {
    let world = pose.world();
    let pivot = pose.pivot(&world, joint);
    let from = pose.effector(&world, effector) - pivot;
    let to = target - pivot;
    if from.length() < EPSILON || to.length() < EPSILON {
        return;
    }
    let delta = Quat::from_rotation_arc(from.normalize(), to.normalize());
    pose.turn(&world, joint, delta);
}

/// `v` with its component along the unit vector `axis` removed.
pub(super) fn reject(v: Vec3, axis: Vec3) -> Vec3 {
    v - axis * v.dot(axis)
}
