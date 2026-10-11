//! Reads a `KeyframeSequence` instance tree into a [`Clip`] and writes one
//! back.
//!
//! The tree is `KeyframeSequence > Keyframe (Time) > Pose > Pose > …`, each
//! `Pose` named after the part its joint moves (the joint's `Part1`) and
//! nested under the pose of that joint's `Part0`, starting from
//! `HumanoidRootPart`. A pose that exists only to hold that nesting together
//! is written with `Weight` 0, which is how Roblox's own editor marks a joint
//! the keyframe does not actually key. Reading skips a weightless pose that
//! has poses under it for the same reason; a weightless leaf is a real key.

use rbx_dom::{Ref, Variant, WeakDom};

use crate::clip::{Clip, Keyframe, Marker, PoseKey, Priority};
use crate::easing::{Direction, Style};
use crate::frame::Frame;
use crate::rig::{cframe, Rig};

fn float(dom: &WeakDom, of: Ref, property: &str) -> Option<f32> {
    match dom.get(of)?.properties().get(property)? {
        Variant::Float32(v) => Some(*v),
        Variant::Float64(v) => Some(*v as f32),
        _ => None,
    }
}

fn enumeration(dom: &WeakDom, of: Ref, property: &str) -> Option<u32> {
    match dom.get(of)?.properties().get(property)? {
        Variant::Enum(v) => Some(*v),
        _ => None,
    }
}

fn text(dom: &WeakDom, of: Ref, property: &str) -> String {
    match dom.get(of).and_then(|i| i.properties().get(property)) {
        Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn children_of<'a>(dom: &'a WeakDom, of: Ref, class: &'a str) -> impl Iterator<Item = Ref> + 'a {
    dom.get(of)
        .map(|i| i.children())
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(move |&c| dom.get(c).is_some_and(|i| i.class() == class))
}

fn read_poses(dom: &WeakDom, of: Ref, into: &mut Keyframe) {
    for pose in children_of(dom, of, "Pose") {
        let weight = float(dom, pose, "Weight").unwrap_or(1.);
        let structural = weight <= 0. && children_of(dom, pose, "Pose").next().is_some();
        if !structural {
            let name = dom.get(pose).map(|i| i.name().to_owned()).unwrap_or_default();
            into.poses.insert(
                name,
                PoseKey {
                    cframe: cframe(dom, pose, "CFrame").unwrap_or_default(),
                    style: Style::from_value(enumeration(dom, pose, "EasingStyle").unwrap_or(0)),
                    direction: Direction::from_value(
                        enumeration(dom, pose, "EasingDirection").unwrap_or(0),
                    ),
                    weight,
                },
            );
        }
        read_poses(dom, pose, into);
    }
}

/// Reads the sequence at `sequence`. Anything that is not a
/// `KeyframeSequence` reads as an empty clip named after it.
pub fn read(dom: &WeakDom, sequence: Ref) -> Clip {
    let Some(instance) = dom.get(sequence) else {
        return Clip::default();
    };
    let mut clip = Clip {
        name: instance.name().to_owned(),
        looped: matches!(instance.properties().get("Loop"), Some(Variant::Bool(true))),
        priority: Priority::from_value(enumeration(dom, sequence, "Priority").unwrap_or(1000)),
        keyframes: Vec::new(),
    };
    for k in children_of(dom, sequence, "Keyframe") {
        let mut keyframe = Keyframe::at(float(dom, k, "Time").unwrap_or(0.));
        keyframe.name = dom.get(k).map(|i| i.name().to_owned()).unwrap_or_default();
        read_poses(dom, k, &mut keyframe);
        keyframe.markers = children_of(dom, k, "KeyframeMarker")
            .map(|m| Marker {
                name: dom.get(m).map(|i| i.name().to_owned()).unwrap_or_default(),
                value: text(dom, m, "Value"),
            })
            .collect();
        // Two Keyframes at the same time are legal in a file; fold them the
        // way the editor's own model keeps them, one per time.
        let target = clip.keyframe_mut(keyframe.time);
        target.poses.append(&mut keyframe.poses);
        target.markers.append(&mut keyframe.markers);
        target.name = keyframe.name;
    }
    clip
}

fn set(dom: &mut WeakDom, of: Ref, property: &str, value: Variant) {
    // Only fails on a referent that is not in the DOM, and every referent
    // here was made a line earlier.
    let _ = dom.set_property(of, property, value);
}

fn write_pose(dom: &mut WeakDom, parent: Ref, name: &str, key: Option<&PoseKey>) -> Ref {
    let pose = dom.new_instance("Pose", name, Some(parent));
    let key = key.copied().unwrap_or(PoseKey {
        weight: 0.,
        ..PoseKey::new(Frame::IDENTITY)
    });
    set(dom, pose, "CFrame", Variant::CFrame(key.cframe.to_data()));
    set(dom, pose, "EasingStyle", Variant::Enum(key.style.value()));
    set(dom, pose, "EasingDirection", Variant::Enum(key.direction.value()));
    set(dom, pose, "Weight", Variant::Float32(key.weight));
    pose
}

/// The chain of part names from the rig's root down to `joint`, root first.
fn chain(rig: &Rig, joint: &str) -> Vec<String> {
    let mut out = vec![joint.to_owned()];
    let mut at = joint.to_owned();
    while let Some(parent) = rig.parent_name(&at) {
        if out.iter().any(|n| n == parent) {
            break;
        }
        out.push(parent.to_owned());
        at = parent.to_owned();
    }
    out.reverse();
    out
}

/// Writes `clip` as a new `KeyframeSequence` under `parent` and returns it.
/// With a rig, poses nest along its joints, as Roblox requires to match them
/// up; without one (or for a joint the rig does not have) they sit directly
/// under the keyframe.
pub fn write(dom: &mut WeakDom, parent: Option<Ref>, clip: &Clip, rig: Option<&Rig>) -> Ref {
    let sequence = dom.new_instance("KeyframeSequence", &clip.name, parent);
    set(dom, sequence, "Loop", Variant::Bool(clip.looped));
    set(dom, sequence, "Priority", Variant::Enum(clip.priority.value()));
    for keyframe in &clip.keyframes {
        let k = dom.new_instance("Keyframe", &keyframe.name, Some(sequence));
        set(dom, k, "Time", Variant::Float32(keyframe.time));
        let mut made: Vec<(String, Ref)> = Vec::new();
        for (joint, key) in &keyframe.poses {
            let path = match rig {
                Some(rig) if rig.joint_for(joint).is_some() => chain(rig, joint),
                _ => vec![joint.clone()],
            };
            let mut under = k;
            for name in &path {
                if let Some((_, existing)) = made.iter().find(|(n, _)| n == name) {
                    under = *existing;
                    continue;
                }
                let own = (name == joint).then_some(key);
                let own = own.or_else(|| keyframe.poses.get(name));
                under = write_pose(dom, under, name, own);
                made.push((name.clone(), under));
            }
        }
        for marker in &keyframe.markers {
            let m = dom.new_instance("KeyframeMarker", &marker.name, Some(k));
            set(dom, m, "Value", Variant::String(marker.value.clone()));
        }
    }
    sequence
}

/// Replaces the children of an existing `KeyframeSequence` with `clip`,
/// keeping the instance itself (its referent, parent, and anything else that
/// points at it).
pub fn overwrite(dom: &mut WeakDom, sequence: Ref, clip: &Clip, rig: Option<&Rig>) {
    let old: Vec<Ref> = dom
        .get(sequence)
        .map(|i| i.children().to_vec())
        .unwrap_or_default();
    for child in old {
        if dom.get(child).is_some_and(|i| i.class() == "Keyframe") {
            dom.remove(child);
        }
    }
    let fresh = write(dom, None, clip, rig);
    let keyframes = dom.get(fresh).map(|i| i.children().to_vec()).unwrap_or_default();
    for k in keyframes {
        dom.set_parent(k, Some(sequence));
    }
    dom.remove(fresh);
    set(dom, sequence, "Loop", Variant::Bool(clip.looped));
    set(dom, sequence, "Priority", Variant::Enum(clip.priority.value()));
    if dom.get(sequence).is_some_and(|i| i.name() != clip.name) {
        let _ = dom.set_name(sequence, &clip.name);
    }
}

#[cfg(test)]
mod tests;
