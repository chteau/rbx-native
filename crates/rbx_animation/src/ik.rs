//! Inverse kinematics for posing a rig by dragging one of its parts, after the
//! Animation Editor's two IK modes. [`chain`] picks the joints a drag moves and
//! [`solve`] returns the new `Transform` map; only those joints' rotations
//! change, so the result keys like any FK edit.
//!
//! - [`Mode::BodyPart`] moves the dragged part's own limb: an R15 hand, lower
//!   arm, foot or lower leg bends its shoulder/hip and elbow/knee through an
//!   analytic two-bone solve (the wrist or ankle keeps its local rotation);
//!   anything else, R6 limbs included, turns its own joint to aim at the target.
//! - [`Mode::FullBody`] runs FABRIK along every joint from the dragged part up
//!   to the root.
//!
//! In both modes a joint is left out when moving it would move a pinned part,
//! which is what keeps pinned parts fixed in world space.
//!
//! Roblox documents neither mode's solver; the chains follow R15 topology and
//! `IKControl`'s documented Arm/Leg bend defaults. In Full Body mode the
//! elbows and knees act as ball joints, so the no-hyperextension rule holds
//! only in Body Part mode.

use std::collections::{BTreeMap, HashSet};

use glam::Vec3;
use rbx_dom::Ref;

use crate::frame::Frame;
use crate::rig::Rig;

mod fabrik;
mod pose;
mod two_bone;

#[cfg(test)]
mod tests;

use pose::Pose;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    #[default]
    BodyPart,
    FullBody,
}

/// The joints (indices into `rig.joints`, root to tip) a drag on `part`
/// turns. A part with nothing to bend below it, such as an upper arm, gets
/// only its own joint, and the rig's root part gets none.
pub fn chain(rig: &Rig, part: Ref, mode: Mode, pinned: &HashSet<Ref>) -> Vec<usize> {
    let parts = match mode {
        Mode::BodyPart => limb(rig, part),
        Mode::FullBody => ancestry(rig, part),
    };
    let blocked = holding_pinned(rig, pinned);
    parts
        .into_iter()
        .filter(|p| !blocked.contains(p))
        .filter_map(|p| rig.joints.iter().position(|j| j.part1 == p))
        .collect()
}

/// Poses the rig so the point `effector_local` (in `part`'s own space) lands
/// on `target` (world space), or as near as the chain reaches. Returns the
/// whole `transforms` map with the chain's joints re-keyed; each keeps its
/// translation, only its rotation changes.
#[allow(clippy::too_many_arguments)]
pub fn solve(
    rig: &Rig,
    root: Frame,
    transforms: &BTreeMap<String, Frame>,
    part: Ref,
    effector_local: Vec3,
    target: Vec3,
    mode: Mode,
    pinned: &HashSet<Ref>,
) -> BTreeMap<String, Frame> {
    let joints = chain(rig, part, mode, pinned);
    let mut pose = Pose::new(rig, root, transforms.clone());
    let effector = (part, effector_local);
    match (mode, joints.as_slice()) {
        (_, []) => {}
        (_, &[joint]) => pose::aim(&mut pose, joint, effector, target),
        (Mode::BodyPart, &[upper, lower]) => {
            let back = bend_side(rig, root, upper);
            two_bone::solve(&mut pose, [upper, lower], effector, target, back);
        }
        _ => fabrik::solve(&mut pose, &joints, effector, target),
    }
    pose.into_transforms()
}

fn parent(rig: &Rig, part: Ref) -> Option<Ref> {
    rig.joint_driving(part).map(|j| j.part0)
}

/// Body Part mode's chain, by R15 naming: a hand or foot bends the two joints
/// above it, a lower arm or leg bends itself and the joint above.
fn limb(rig: &Rig, part: Ref) -> Vec<Ref> {
    let name = rig.part_name(part);
    if name.ends_with("Hand") || name.ends_with("Foot") {
        let lower = parent(rig, part);
        let upper = lower.and_then(|p| parent(rig, p));
        upper.into_iter().chain(lower).collect()
    } else if name.ends_with("LowerArm") || name.ends_with("LowerLeg") {
        parent(rig, part).into_iter().chain([part]).collect()
    } else {
        vec![part]
    }
}

/// `part` and every part above it that a joint drives, root first.
fn ancestry(rig: &Rig, part: Ref) -> Vec<Ref> {
    let mut parts = vec![part];
    // Bounded by the joint count so a malformed, cyclic rig cannot hang.
    while parts.len() <= rig.joints.len() {
        match parent(rig, parts[parts.len() - 1]) {
            Some(up) if rig.joint_driving(up).is_some() => parts.push(up),
            _ => break,
        }
    }
    parts.reverse();
    parts
}

/// Every pinned part and all its ancestors: turning any of their joints
/// would carry a pinned part along.
fn holding_pinned(rig: &Rig, pinned: &HashSet<Ref>) -> HashSet<Ref> {
    pinned.iter().flat_map(|&p| ancestry(rig, p)).collect()
}

/// The side an elbow or knee is allowed to bend toward, in the upper limb's
/// own space. Characters face -Z, so at rest elbows point back (+Z) and knees
/// forward (-Z); fixing it to the limb rather than the body keeps it right
/// however the limb is raised or twisted.
fn bend_side(rig: &Rig, root: Frame, upper: usize) -> Vec3 {
    let part = rig.joints[upper].part1;
    let sign = if rig.part_name(part).contains("Leg") {
        -1.
    } else {
        1.
    };
    let rest = rig.layout(root, &|_| None);
    let rest_rotation = rest.get(&part).map_or(root.rotation, |f| f.rotation);
    rest_rotation.conjugate() * (root.rotation * Vec3::new(0., 0., sign))
}
