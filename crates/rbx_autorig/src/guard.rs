//! Where a hand-moved joint may go. Nothing else limits a marker drag, and a
//! knee dragged up to the neck cuts a leg off the body; [`guard`] keeps every
//! joint in the order its chain runs (hip, knee, ankle, foot) and inside the
//! mesh, so any drop still cuts into 15 attached parts.

use glam::Vec3;

use crate::fit::Fit;
use crate::mesh::{bounds, Mesh};
use crate::rig::{Joint, Side};

/// How close, as a share of the segment it sits on, a joint may come to the
/// joint before or after it on its chain.
const MARGIN: f32 = 0.1;
/// How far a mid-chain joint may stand off the line between its neighbours,
/// as a share of that line's length: a bent knee or elbow, not a joint
/// dragged onto another limb.
const SIDEWAYS: f32 = 0.5;

/// The mesh's bounds in Roblox space under `fit`, which [`guard`] keeps a
/// joint inside.
pub fn mesh_bounds(mesh: &Mesh, fit: &Fit) -> (Vec3, Vec3) {
    bounds(mesh.positions.iter().map(|&p| fit.to_roblox(p)))
}

/// The tip past a chain's last joint: the head past the neck, a hand past
/// its wrist, a foot past its ankle.
fn tip(fit: &Fit, joint: Joint) -> Option<Vec3> {
    let e = &fit.extremities;
    Some(match joint {
        Joint::Neck => e.head,
        Joint::LeftWrist => e.hand(Side::Left),
        Joint::RightWrist => e.hand(Side::Right),
        Joint::LeftAnkle => e.foot(Side::Left),
        Joint::RightAnkle => e.foot(Side::Right),
        _ => return None,
    })
}

/// The next joint down the chain (towards the tip).
fn child(joint: Joint) -> Option<Joint> {
    Some(match joint {
        Joint::Root => Joint::Waist,
        Joint::Waist => Joint::Neck,
        Joint::LeftShoulder => Joint::LeftElbow,
        Joint::LeftElbow => Joint::LeftWrist,
        Joint::RightShoulder => Joint::RightElbow,
        Joint::RightElbow => Joint::RightWrist,
        Joint::LeftHip => Joint::LeftKnee,
        Joint::LeftKnee => Joint::LeftAnkle,
        Joint::RightHip => Joint::RightKnee,
        Joint::RightKnee => Joint::RightAnkle,
        _ => return None,
    })
}

/// Keeps `at` on the far side of the plane through `from` that faces
/// `towards`, by at least `MARGIN` of the distance between them: a joint
/// that may not pass `from` going `towards`.
fn short_of(at: Vec3, from: Vec3, towards: Vec3) -> Vec3 {
    let axis = towards - from;
    let len = axis.length();
    if len < 1e-6 {
        return at;
    }
    let dir = axis / len;
    let past = (at - from).dot(dir) + MARGIN * len;
    if past > 0. {
        at - dir * past
    } else {
        at
    }
}

/// Where `joint` stands when it is asked to go to `to`, given the rest of
/// `fit` and the mesh's `bounds` (from [`mesh_bounds`]):
///
/// - inside the bounds;
/// - a mid-chain joint (waist, neck, elbow, wrist, knee, ankle) between the
///   joint before it and the one after it (or the tip), no nearer either
///   than a tenth of that span, and no further off the line between them
///   than half of it;
/// - a shoulder or hip short of its elbow or knee, on the side away from
///   the wrist or ankle, and on its own side of the centre line;
/// - the root short of the waist, and above the knees.
///
/// A fitted skeleton already meets all of these, so the guard leaves an
/// untouched joint where it is.
pub fn guard(fit: &Fit, joint: Joint, to: Vec3, bounds: (Vec3, Vec3)) -> Vec3 {
    let mut at = to.clamp(bounds.0, bounds.1);
    let parent = joint.parent_part().map(|p| fit.joint(p.joint()));
    let next = child(joint)
        .map(|c| fit.joint(c))
        .or_else(|| tip(fit, joint));
    match joint {
        Joint::Root => {
            let waist = fit.joint(Joint::Waist);
            at = short_of(at, waist, fit.joint(Joint::Neck) * 2. - waist);
            let knees = fit
                .joint(Joint::LeftKnee)
                .y
                .max(fit.joint(Joint::RightKnee).y);
            at.y = at.y.max(knees + MARGIN * (waist.y - knees).max(0.));
        }
        Joint::LeftShoulder | Joint::RightShoulder | Joint::LeftHip | Joint::RightHip => {
            let next = fit.joint(child(joint).expect("a limb's first joint has a child"));
            let after = fit.joint(child(child(joint).unwrap()).unwrap());
            at = short_of(at, next, after);
            // Never across the centre line onto the other limb.
            let sign = joint.side().map_or(1., Side::sign);
            if at.x * sign < 0. {
                at.x = 0.;
            }
        }
        _ => {
            if let (Some(parent), Some(next)) = (parent, next) {
                let axis = next - parent;
                let len2 = axis.length_squared();
                if len2 > 1e-10 {
                    let t = (at - parent).dot(axis) / len2;
                    let off = (at - (parent + axis * t)).clamp_length_max(SIDEWAYS * len2.sqrt());
                    at = parent + axis * t.clamp(MARGIN, 1. - MARGIN) + off;
                }
            }
        }
    }
    at.clamp(bounds.0, bounds.1)
}
