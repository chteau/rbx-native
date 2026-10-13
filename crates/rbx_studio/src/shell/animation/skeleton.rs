//! The skeleton view: each joint of the bound rig drawn over the 3D view
//! as one of Blender's octahedral bones. A bone runs from its joint's pivot
//! (the head) to the next joint down the chain (the tail), widest a fifth of
//! the way along. A joint with no joint below it (a hand, a foot, the head)
//! ends as far past its part's centre as the pivot sits before it. Bones
//! draw on top of the rig, so they show through it as Blender's "In Front"
//! does, with the selected joint's bone in the accent colour.

use std::collections::HashMap;

use crate::tokens;
use glam::Vec3;
use rbx_animation::{Frame, Rig};
use rbx_dom::Ref;
use rbx_viewer::Segment;

/// Where the bone is widest, as a share of its length, and how wide it is
/// there, as a share of its length too (Blender's own proportions).
const WAIST: f32 = 0.2;
const GIRTH: f32 = 0.1;

/// A bone's light grey, as linear RGBA. A theme can change it, so the active
/// palette is read on each call.
fn bone_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_bone())
}

/// The selected bone's blue, the accent the rest of the editor selects with.
/// Read from the active palette on each call, as [`bone_color`] is.
fn selected_bone_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_bone_selected())
}

/// One bone: where its joint is, where it points, and whether it is the
/// joint being edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bone {
    pub(super) head: Vec3,
    pub(super) tail: Vec3,
    pub(super) selected: bool,
}

/// The bones of `rig` laid out as `world` places its parts.
pub(super) fn bones(rig: &Rig, world: &HashMap<Ref, Frame>, selected: Option<&str>) -> Vec<Bone> {
    let head_of = |j: usize| {
        let joint = &rig.joints[j];
        world.get(&joint.part0).map(|p0| joint.pivot(p0).position)
    };
    (0..rig.joints.len())
        .filter_map(|j| {
            let joint = &rig.joints[j];
            let head = head_of(j)?;
            let below = rig
                .joints
                .iter()
                .enumerate()
                .filter(|(_, c)| c.part0 == joint.part1)
                .filter_map(|(k, _)| head_of(k))
                .collect::<Vec<_>>();
            let tail = if below.is_empty() {
                let centre = world.get(&joint.part1)?.position;
                centre * 2.0 - head
            } else {
                // Several joints below (a torso's neck and shoulders): the
                // bone points between them.
                below.iter().copied().sum::<Vec3>() / below.len() as f32
            };
            Some(Bone {
                head,
                tail,
                selected: selected == Some(rig.part_name(joint.part1)),
            })
        })
        .collect()
}

/// The line segments that draw `bones`: four edges out from the head to
/// the waist, four on to the tail, the waist's square, and a dot on the
/// joint.
pub(super) fn segments(bones: &[Bone]) -> Vec<Segment> {
    let mut out = Vec::with_capacity(bones.len() * 13);
    for bone in bones {
        let axis = bone.tail - bone.head;
        let length = axis.length();
        if length < 1e-4 {
            continue;
        }
        let dir = axis / length;
        let (a, b) = dir.any_orthonormal_pair();
        let waist = bone.head + axis * WAIST;
        let r = length * GIRTH;
        let ring = [a, b, -a, -b].map(|d| waist + d * r);
        let colour = if bone.selected {
            selected_bone_color()
        } else {
            bone_color()
        };
        let line = |from: Vec3, to: Vec3, width: f32| Segment {
            from,
            to,
            color: colour,
            on_top: true,
            width,
        };
        for (i, &corner) in ring.iter().enumerate() {
            out.push(line(bone.head, corner, 1.25));
            out.push(line(corner, bone.tail, 1.25));
            out.push(line(corner, ring[(i + 1) % 4], 1.25));
        }
        out.push(line(bone.head, bone.head, 6.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bone_is_an_octahedron_from_head_to_tail() {
        let bone = Bone {
            head: Vec3::ZERO,
            tail: Vec3::new(0.0, 2.0, 0.0),
            selected: false,
        };
        let lines = segments(&[bone]);
        assert_eq!(lines.len(), 13);
        // The waist sits a fifth of the way up, a tenth of the length out.
        let corner = lines[0].to;
        assert!((corner.y - 0.4).abs() < 1e-5);
        assert!((glam::Vec2::new(corner.x, corner.z).length() - 0.2).abs() < 1e-5);
        assert!(lines.iter().all(|s| s.on_top));
        // The joint's dot.
        let dot = lines.last().unwrap();
        assert_eq!((dot.from, dot.to), (Vec3::ZERO, Vec3::ZERO));
    }

    #[test]
    fn a_zero_length_bone_draws_nothing_and_selection_is_coloured() {
        let still = Bone {
            head: Vec3::ONE,
            tail: Vec3::ONE,
            selected: false,
        };
        assert!(segments(&[still]).is_empty());
        let picked = Bone {
            head: Vec3::ZERO,
            tail: Vec3::X,
            selected: true,
        };
        assert!(segments(&[picked])
            .iter()
            .all(|s| s.color == selected_bone_color()));
    }
}
