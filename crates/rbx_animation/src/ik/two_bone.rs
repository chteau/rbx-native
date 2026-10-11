//! The analytic two-bone solve for an arm or leg.
//!
//! The elbow (or knee) is treated as a hinge: it only ever turns about the
//! normal of its current bend plane, by whatever brings the shoulder-to-
//! effector distance to the target's (law of cosines). The shoulder then
//! swings that whole limb onto the target while keeping the elbow on the same
//! side of the shoulder-target line as before, so the bend plane carries over.

use std::f32::consts::PI;

use glam::{Mat3, Quat, Vec3};

use super::pose::{reject, Effector, Pose, EPSILON};

/// Bends sharper than this one way or the other count as bent, not straight.
const STRAIGHT: f32 = 1e-4;

/// `back` is the side the elbow may point to, in the upper limb's own space.
pub(super) fn solve(
    pose: &mut Pose,
    [upper, lower]: [usize; 2],
    effector: Effector,
    target: Vec3,
    back: Vec3,
) {
    let world = pose.world();
    let shoulder = pose.pivot(&world, upper);
    let elbow = pose.pivot(&world, lower);
    let hand = pose.effector(&world, effector);
    let (l1, l2) = ((elbow - shoulder).length(), (hand - elbow).length());
    if l1 < EPSILON || l2 < EPSILON {
        // A grab on the elbow's own pivot leaves nothing for it to bend.
        return super::pose::aim(pose, upper, effector, target);
    }
    let (u1, u2) = ((elbow - shoulder) / l1, (hand - elbow) / l2);

    let back = world
        .get(&pose.part1(upper))
        .map_or(back, |f| f.rotation * back);
    // Turning the forearm about this axis swings the hand forward, which is
    // the elbow pointing back: the only way a hinge joint may bend.
    let allowed = back.cross(u1);
    let bent = u1.cross(u2);
    let (axis, current) = if bent.length() > STRAIGHT {
        let bend = u1.angle_between(u2);
        if bent.dot(allowed) < 0. {
            // Hyperextended: keep the plane but measure the bend as negative,
            // so the solve swings it back through straight to the right side.
            (-bent.normalize(), -bend)
        } else {
            (bent.normalize(), bend)
        }
    } else if allowed.length() > EPSILON {
        (allowed.normalize(), 0.)
    } else {
        (u1.any_orthonormal_vector(), 0.)
    };

    let reach = (target - shoulder).length().clamp((l1 - l2).abs(), l1 + l2);
    let cos_inner = ((l1 * l1 + l2 * l2 - reach * reach) / (2. * l1 * l2)).clamp(-1., 1.);
    let wanted = PI - cos_inner.acos();
    pose.turn(&world, lower, Quat::from_axis_angle(axis, wanted - current));

    // The side the elbow sits on, perpendicular to the upper bone.
    let pole = u1.cross(axis);
    let world = pose.world();
    let hand = pose.effector(&world, effector);
    let Some(line) = (hand - shoulder).try_normalize() else {
        return;
    };
    let Some(to_line) = (target - shoulder).try_normalize() else {
        return;
    };
    let side = reject(elbow - shoulder, line)
        .try_normalize()
        .unwrap_or_else(|| reject(pole, line).normalize_or(line.any_orthonormal_vector()));
    let to_side = reject(pole, to_line)
        .try_normalize()
        .unwrap_or_else(|| Quat::from_rotation_arc(line, to_line) * side);
    let delta = basis(to_line, to_side) * basis(line, side).conjugate();
    pose.turn(&world, upper, delta.normalize());
}

/// The rotation taking X to `x` and Y to `y` (unit and perpendicular, or
/// near enough that re-orthonormalising `y` is harmless).
fn basis(x: Vec3, y: Vec3) -> Quat {
    let y = reject(y, x).normalize();
    Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)))
}
