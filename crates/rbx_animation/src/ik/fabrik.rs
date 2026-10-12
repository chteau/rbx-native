//! FABRIK over a chain of joint pivots ending at the grabbed point, then each
//! solved bone direction turned back into its joint's rotation.
//!
//! The nodes are the chain's pivots root to tip, then the effector. Successive
//! pivots sit on one rigid part each, so the distances between them are the
//! bone lengths FABRIK must keep.

use glam::{Quat, Vec3};

use super::pose::{Effector, Pose, EPSILON};

/// Studs: well under what a drag in the viewport can resolve.
const TOLERANCE: f32 = 1e-3;
const ITERATIONS: usize = 20;

pub(super) fn solve(pose: &mut Pose, chain: &[usize], effector: Effector, target: Vec3) {
    let world = pose.world();
    let mut nodes: Vec<Vec3> = chain.iter().map(|&j| pose.pivot(&world, j)).collect();
    nodes.push(pose.effector(&world, effector));
    let lengths: Vec<f32> = nodes.windows(2).map(|w| (w[1] - w[0]).length()).collect();
    reach(&mut nodes, &lengths, target);

    // Root first: each turn moves only the nodes past it, and leaves the
    // next joint's pivot where the solve put it.
    for (k, &joint) in chain.iter().enumerate() {
        let world = pose.world();
        let pivot = pose.pivot(&world, joint);
        let next = match chain.get(k + 1) {
            Some(&j) => pose.pivot(&world, j),
            None => pose.effector(&world, effector),
        };
        let (Some(from), Some(to)) = (
            (next - pivot).try_normalize(),
            (nodes[k + 1] - pivot).try_normalize(),
        ) else {
            continue;
        };
        // Shortest arc, so the bone keeps whatever twist it had.
        pose.turn(&world, joint, Quat::from_rotation_arc(from, to));
    }
}

/// Moves `nodes` (base fixed) so the last lands on `target`, or points the
/// chain straight at it when it is out of reach.
fn reach(nodes: &mut [Vec3], lengths: &[f32], target: Vec3) {
    let base = nodes[0];
    let total: f32 = lengths.iter().sum();
    if (target - base).length() >= total {
        let dir = (target - base).normalize_or_zero();
        for (i, &l) in lengths.iter().enumerate() {
            nodes[i + 1] = nodes[i] + dir * l;
        }
        return;
    }
    let tip = nodes.len() - 1;
    for _ in 0..ITERATIONS {
        if (nodes[tip] - target).length() < TOLERANCE {
            break;
        }
        nodes[tip] = target;
        for i in (0..tip).rev() {
            nodes[i] = towards(nodes[i + 1], nodes[i], lengths[i]);
        }
        nodes[0] = base;
        for i in 0..tip {
            nodes[i + 1] = towards(nodes[i], nodes[i + 1], lengths[i]);
        }
    }
}

/// The point `length` from `from` in the direction of `to`; a coincident pair
/// has no direction, so it stays put rather than going NaN.
fn towards(from: Vec3, to: Vec3, length: f32) -> Vec3 {
    let d = to - from;
    if d.length() < EPSILON {
        return to;
    }
    from + d.normalize() * length
}
