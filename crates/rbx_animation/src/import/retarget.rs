//! Retargets a file's skeletal animation onto a rig.
//!
//! **Matching.** A bone is matched to a joint by the joint's `Part1` name,
//! or by a known alias. Mixamo's `mixamorig:LeftForeArm` matches
//! `LeftLowerArm` on R15 and its `LeftArm` matches `Left Arm` on R6. Names
//! compare without a `prefix:`, case, spaces or underscores.
//!
//! **World.** The file's world is fitted onto the rig's with a best-fit
//! rotation and scale (`skeleton::fit`) over the matched joints. Arms and
//! hands are left out of the fit, because a T-pose and Roblox's arms-down
//! rest disagree about where they are and would skew it.
//!
//! **Rest pose.** Each limb's rest direction in the file (bone head to its
//! matched child's head) is aligned with the rig's (joint pivot to its
//! child's pivot). That is what lets a T-pose Mixamo walk drive an
//! arms-down Roblox rig.
//!
//! **Motion.** Every frame takes each matched bone's animated world
//! rotation relative to its own rest, so bones the rig has no joint for
//! (Mixamo's three spine bones under one `UpperTorso`) still pass their
//! turn on. Only the joint off the root part (the hips) keeps translation,
//! which is the body's bob and travel.

use std::collections::HashMap;

use glam::{Quat, Vec3};

use crate::clip::{Clip, PoseKey};
use crate::frame::Frame;
use crate::import::skeleton::{fit, Fit, Skeleton};
use crate::rig::Rig;

/// Rig part → the bone names other tools give it, tried after an exact
/// match. Mixamo first, then the common Blender/Unreal spellings.
const ALIASES: [(&str, &[&str]); 21] = [
    ("LowerTorso", &["Hips", "Pelvis"]),
    (
        "UpperTorso",
        &["Spine2", "Chest", "UpperChest", "Spine1", "Spine03"],
    ),
    ("Head", &["Head"]),
    (
        "LeftUpperArm",
        &["LeftArm", "UpperArmL", "LeftShoulderArm", "upperarm_l"],
    ),
    ("LeftLowerArm", &["LeftForeArm", "ForeArmL", "lowerarm_l"]),
    ("LeftHand", &["LeftHand", "HandL", "hand_l"]),
    ("RightUpperArm", &["RightArm", "UpperArmR", "upperarm_r"]),
    ("RightLowerArm", &["RightForeArm", "ForeArmR", "lowerarm_r"]),
    ("RightHand", &["RightHand", "HandR", "hand_r"]),
    (
        "LeftUpperLeg",
        &["LeftUpLeg", "LeftThigh", "ThighL", "thigh_l"],
    ),
    (
        "LeftLowerLeg",
        &["LeftLeg", "LeftShin", "LeftCalf", "ShinL", "calf_l"],
    ),
    ("LeftFoot", &["LeftFoot", "FootL", "foot_l"]),
    (
        "RightUpperLeg",
        &["RightUpLeg", "RightThigh", "ThighR", "thigh_r"],
    ),
    (
        "RightLowerLeg",
        &["RightLeg", "RightShin", "RightCalf", "ShinR", "calf_r"],
    ),
    ("RightFoot", &["RightFoot", "FootR", "foot_r"]),
    // R6: one part per limb, so the upper bone drives it.
    ("Torso", &["Hips", "Pelvis"]),
    ("Left Arm", &["LeftArm", "UpperArmL", "upperarm_l"]),
    ("Right Arm", &["RightArm", "UpperArmR", "upperarm_r"]),
    ("Left Leg", &["LeftUpLeg", "LeftThigh", "ThighL", "thigh_l"]),
    (
        "Right Leg",
        &["RightUpLeg", "RightThigh", "ThighR", "thigh_r"],
    ),
    ("HumanoidRootPart", &["HumanoidRootPart"]),
];

/// `mixamorig:Left_Fore Arm` → `leftforearm`.
pub fn normalise(name: &str) -> String {
    let bare = name.rsplit(':').next().unwrap_or(name);
    bare.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The bone (by index) that drives rig part `part`, if any.
pub fn bone_for(index: &HashMap<String, usize>, part: &str) -> Option<usize> {
    if let Some(&b) = index.get(&normalise(part)) {
        return Some(b);
    }
    let aliases = ALIASES.iter().find(|(p, _)| *p == part)?.1;
    aliases
        .iter()
        .find_map(|a| index.get(&normalise(a)).copied())
}

fn shortest_arc(from: Vec3, to: Vec3) -> Quat {
    if from.length_squared() < 1e-10 || to.length_squared() < 1e-10 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(from.normalize(), to.normalize())
}

fn limb(name: &str) -> bool {
    let n = normalise(name);
    n.contains("arm") || n.contains("hand") || n.contains("shoulder")
}

/// Turns the file's animation into a clip on `rig`, whose root stands at
/// `root`. The root part itself is never keyed: an animation plays in place
/// of wherever the character stands.
pub fn retarget(skeleton: &Skeleton, rig: &Rig, root: Frame, name: &str) -> Clip {
    let mut clip = Clip {
        name: name.to_owned(),
        ..Clip::default()
    };
    let index: HashMap<String, usize> = skeleton
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| (normalise(&b.name), i))
        .collect();
    let rest = skeleton.rest_world();
    let parts = rig.layout(root, &|_| None);

    // The joints a bone drives, in the rig's parents-first order.
    let matched: Vec<(usize, usize)> = rig
        .joints
        .iter()
        .enumerate()
        .filter_map(|(j, joint)| bone_for(&index, rig.part_name(joint.part1)).map(|b| (j, b)))
        .collect();
    if matched.is_empty() {
        return clip;
    }
    let bone_of = |j: usize| matched.iter().find(|(k, _)| *k == j).map(|(_, b)| *b);
    let pivot = |j: usize| {
        let joint = &rig.joints[j];
        parts
            .get(&joint.part0)
            .map(|p0| joint.pivot(p0))
            .unwrap_or_default()
    };

    let (mut from, mut to) = (Vec::new(), Vec::new());
    for &(j, b) in &matched {
        if !limb(rig.part_name(rig.joints[j].part1)) {
            from.push(rest[b].0.position);
            to.push(pivot(j).position);
        }
    }
    let world = fit(&from, &to)
        .or_else(|| {
            let from: Vec<Vec3> = matched.iter().map(|&(_, b)| rest[b].0.position).collect();
            let to: Vec<Vec3> = matched.iter().map(|&(j, _)| pivot(j).position).collect();
            fit(&from, &to)
        })
        .unwrap_or(Fit {
            rotation: Quat::IDENTITY,
            scale: 1.0,
        });
    let g = world.rotation;
    let into_rig = |v: Vec3| g * v * world.scale;

    // Per matched joint: how to turn the rig's rest limb onto the file's
    // rest limb, measured along each one's first matched child.
    let mut align: HashMap<usize, Quat> = HashMap::new();
    for &(j, b) in &matched {
        let part1 = rig.joints[j].part1;
        let children: Vec<usize> = rig
            .joints
            .iter()
            .enumerate()
            .filter(|(_, c)| c.part0 == part1)
            .map(|(k, _)| k)
            .collect();
        let pairs: Vec<(Vec3, Vec3)> = children
            .iter()
            .filter_map(|&k| {
                let cb = bone_of(k)?;
                Some((
                    pivot(k).position - pivot(j).position,
                    into_rig(rest[cb].0.position - rest[b].0.position),
                ))
            })
            .collect();
        let turn = match pairs.as_slice() {
            [] => Quat::IDENTITY,
            [(rig_dir, file_dir)] => shortest_arc(*rig_dir, *file_dir),
            _ => {
                let rig_dir: Vec3 = pairs.iter().map(|p| p.0).sum();
                let file_dir: Vec3 = pairs.iter().map(|p| p.1).sum();
                shortest_arc(rig_dir, file_dir)
            }
        };
        align.insert(j, turn);
    }

    // A bone's own keys, or those of a bone between it and the bone its
    // parent joint follows, move it.
    let driven = |j: usize, b: usize| {
        let stop = rig
            .joints
            .iter()
            .position(|p| p.part1 == rig.joints[j].part0)
            .and_then(bone_of);
        let mut at = Some(b);
        while let Some(i) = at {
            if Some(i) == stop {
                return false;
            }
            if !skeleton.bones[i].keys.is_empty() {
                return true;
            }
            at = skeleton.bones[i].parent;
        }
        false
    };
    let keyed: Vec<(usize, usize)> = matched
        .iter()
        .copied()
        .filter(|&(j, b)| driven(j, b))
        .collect();
    if keyed.is_empty() {
        return clip;
    }

    for time in skeleton.times() {
        let animated = skeleton.world_at(time);
        // World rotation of every rig part, parents first, as the file has it.
        let mut turned: HashMap<rbx_dom::Ref, Quat> = HashMap::new();
        if let Some(r) = rig.root {
            turned.insert(r, root.rotation);
        }
        for (j, joint) in rig.joints.iter().enumerate() {
            let Some(&parent) = turned.get(&joint.part0) else {
                continue;
            };
            let rest1 = parts
                .get(&joint.part1)
                .map_or(Quat::IDENTITY, |f| f.rotation);
            let rest0 = parts
                .get(&joint.part0)
                .map_or(Quat::IDENTITY, |f| f.rotation);
            let rotation = match bone_of(j) {
                Some(b) => {
                    let delta =
                        g * (animated[b].rotation * rest[b].0.rotation.conjugate()) * g.conjugate();
                    delta * align[&j] * rest1
                }
                None => parent * rest0.conjugate() * rest1,
            };
            turned.insert(joint.part1, rotation.normalize());
        }
        for &(j, b) in &keyed {
            let joint = &rig.joints[j];
            let (Some(w0), Some(w1)) = (turned.get(&joint.part0), turned.get(&joint.part1)) else {
                continue;
            };
            let rotation =
                (joint.c0.rotation.conjugate() * w0.conjugate() * *w1 * joint.c1.rotation)
                    .normalize();
            let position = if Some(joint.part0) == rig.root {
                let shift = into_rig(animated[b].position - rest[b].0.position);
                (*w0 * joint.c0.rotation).conjugate() * shift
            } else {
                Vec3::ZERO
            };
            clip.set_pose(
                time,
                rig.part_name(joint.part1),
                PoseKey::new(Frame::new(position, rotation)),
            );
        }
    }
    clip
}

#[cfg(test)]
mod tests;
