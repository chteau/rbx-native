//! A skeletal animation as a 3D file holds it (bones, a rest pose, keyed local
//! transforms), and its retargeting onto a rig's joints.
//!
//! A file bone's frame seldom matches the joint's frame. Blender's bones point
//! along their own Y axis, the file may be Z-up or Y-up, and it may be in
//! metres or centimetres. So the motion is not copied across axis by axis.
//! Each key becomes a world-space turn and shift of the bone about its rest
//! position. The file's world is fitted onto the rig's (a best-fit rotation
//! and scale over the matched bones' rest positions, Horn's method). Each
//! joint's `Transform` is then that motion re-expressed in the joint's own
//! frame. This is exact whenever a bone's head sits on its joint's pivot,
//! which is what a rig made for Roblox has.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::clip::{Clip, PoseKey};
use crate::frame::Frame;
use crate::rig::Rig;

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Relative to the parent, in the parent's units.
    pub rest: Frame,
    /// Uniform scale this bone applies to everything under it.
    pub scale: f32,
    /// Keyed local transforms (replacing `rest`), in time order.
    pub keys: Vec<(f32, Frame)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

impl Skeleton {
    /// The local transform of `bone` at `time`, interpolating between keys.
    pub fn local(&self, bone: usize, time: f32) -> Frame {
        let b = &self.bones[bone];
        let Some(first) = b.keys.first() else {
            return b.rest;
        };
        if time <= first.0 {
            return first.1;
        }
        for pair in b.keys.windows(2) {
            let ((t0, f0), (t1, f1)) = (pair[0], pair[1]);
            if time <= t1 {
                let span = t1 - t0;
                let alpha = if span > 0. { (time - t0) / span } else { 1. };
                return f0.lerp(&f1, alpha);
            }
        }
        b.keys.last().map_or(b.rest, |k| k.1)
    }

    /// World rest frames (scale folded into positions) and each bone's world
    /// scale.
    fn rest_world(&self) -> Vec<(Frame, f32)> {
        let mut out: Vec<(Frame, f32)> = Vec::with_capacity(self.bones.len());
        for (i, b) in self.bones.iter().enumerate() {
            let (parent, scale) = match b.parent {
                Some(p) if p < i => out[p],
                _ => (Frame::IDENTITY, 1.),
            };
            let world = Frame::new(
                parent.position + parent.rotation * (b.rest.position * scale),
                (parent.rotation * b.rest.rotation).normalize(),
            );
            out.push((world, scale * b.scale));
        }
        out
    }

    pub fn times(&self) -> Vec<f32> {
        let mut times: Vec<f32> = self
            .bones
            .iter()
            .flat_map(|b| b.keys.iter().map(|k| k.0))
            .collect();
        times.sort_by(f32::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < crate::clip::TIME_EPSILON);
        times
    }
}

/// A rotation and uniform scale taking file-world vectors to rig-world ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub rotation: Quat,
    pub scale: f32,
}

/// Horn's closed-form best rotation (and RMS-ratio scale) taking the centred
/// `from` points onto the centred `to` points. `None` when there are too few
/// points or they all coincide. Collinear points leave the roll about their
/// line free; a rig's joints never are.
pub fn fit(from: &[Vec3], to: &[Vec3]) -> Option<Fit> {
    if from.len() < 3 || from.len() != to.len() {
        return None;
    }
    let n = from.len() as f32;
    let (ca, cb) = (
        from.iter().sum::<Vec3>() / n,
        to.iter().sum::<Vec3>() / n,
    );
    let mut s = [[0f32; 3]; 3];
    let (mut spread_a, mut spread_b) = (0., 0.);
    for (a, b) in from.iter().zip(to) {
        let (a, b) = (*a - ca, *b - cb);
        spread_a += a.length_squared();
        spread_b += b.length_squared();
        let (a, b) = (a.to_array(), b.to_array());
        for (i, row) in s.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell += a[i] * b[j];
            }
        }
    }
    if spread_a < 1e-8 || spread_b < 1e-8 {
        return None;
    }
    let [[sxx, sxy, sxz], [syx, syy, syz], [szx, szy, szz]] = s;
    let n = Mat4::from_cols(
        Vec4::new(sxx + syy + szz, syz - szy, szx - sxz, sxy - syx),
        Vec4::new(syz - szy, sxx - syy - szz, sxy + syx, szx + sxz),
        Vec4::new(szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy),
        Vec4::new(sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz),
    );
    // The best rotation is N's eigenvector of largest eigenvalue. Shift N to
    // positive definite so power iteration converges on that one.
    let shift = n.to_cols_array().iter().map(|v| v.abs()).sum::<f32>();
    let m = n + Mat4::from_diagonal(Vec4::splat(shift));
    let mut q = Vec4::new(1., 0.1, 0.1, 0.1).normalize();
    for _ in 0..200 {
        let next = (m * q).normalize();
        if next.abs_diff_eq(q, 1e-7) {
            q = next;
            break;
        }
        q = next;
    }
    // Horn's quaternion is (w, x, y, z).
    let rotation = Quat::from_xyzw(q.y, q.z, q.w, q.x).normalize();
    Some(Fit {
        rotation,
        scale: (spread_b / spread_a).sqrt(),
    })
}

/// Turns the file's animation into a clip on `rig`, with the rig's root at
/// `root`. Bones are matched to joints by the joint's `Part1` name. Bones
/// with no matching joint, and the root bone, are left out: an animation
/// plays in place.
pub fn retarget(skeleton: &Skeleton, rig: &Rig, root: Frame, name: &str) -> Clip {
    let mut clip = Clip {
        name: name.to_owned(),
        ..Clip::default()
    };
    let rest = skeleton.rest_world();
    let parts = rig.layout(root, &|_| None);
    let index: HashMap<&str, usize> = skeleton
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.as_str(), i))
        .collect();

    // Bone heads against the joints' pivots (and the root bone against the
    // root part) give the world fit.
    let mut matched = Vec::new();
    let (mut from, mut to) = (Vec::new(), Vec::new());
    for joint in &rig.joints {
        let Some(&bone) = index.get(rig.part_name(joint.part1)) else {
            continue;
        };
        let Some(part0) = parts.get(&joint.part0) else {
            continue;
        };
        let pivot = joint.pivot(part0);
        from.push(rest[bone].0.position);
        to.push(pivot.position);
        matched.push((joint, bone, pivot));
    }
    if let (Some(root_ref), Some(&bone)) = (
        rig.root,
        rig.root.and_then(|r| index.get(rig.part_name(r))),
    ) {
        if let Some(part) = parts.get(&root_ref) {
            from.push(rest[bone].0.position);
            to.push(part.position);
        }
    }
    let world = fit(&from, &to).unwrap_or(Fit {
        rotation: Quat::IDENTITY,
        scale: 1.,
    });

    for time in skeleton.times() {
        for (joint, bone, pivot) in &matched {
            let b = &skeleton.bones[*bone];
            if b.keys.is_empty() {
                continue;
            }
            let (parent, parent_scale) = match b.parent {
                Some(p) => rest[p],
                None => (Frame::IDENTITY, 1.),
            };
            let posed = skeleton.local(*bone, time);
            // The bone's motion in the file's world, about its rest head.
            let turn = parent.rotation * posed.rotation * b.rest.rotation.conjugate()
                * parent.rotation.conjugate();
            let shift = parent.rotation * ((posed.position - b.rest.position) * parent_scale);
            // Into the rig's world, then into the joint's frame.
            let turn = world.rotation * turn * world.rotation.conjugate();
            let shift = world.rotation * shift * world.scale;
            let inverse = pivot.rotation.conjugate();
            let transform = Frame::new(
                inverse * shift,
                (inverse * turn * pivot.rotation).normalize(),
            );
            clip.set_pose(time, rig.part_name(joint.part1), PoseKey::new(transform));
        }
    }
    clip
}

/// Drops keys that lie on the straight interpolation of their neighbours,
/// the Animation Editor's *Optimize Keyframes*. A baked file has a key on
/// every frame. Keeping only the ones that change the motion makes the clip
/// editable. `tolerance` is in studs and radians.
pub fn optimize(clip: &mut Clip, tolerance: f32) {
    for joint in clip.joints() {
        let keys: Vec<(f32, PoseKey)> = clip.track(&joint).map(|(t, k)| (t, *k)).collect();
        if keys.len() < 3 {
            continue;
        }
        let mut keep = vec![true; keys.len()];
        let mut anchor = 0;
        for i in 1..keys.len() - 1 {
            let (t0, k0) = keys[anchor];
            let (t1, k1) = keys[i + 1];
            let alpha = (keys[i].0 - t0) / (t1 - t0).max(f32::EPSILON);
            let guess = k0.cframe.lerp(&k1.cframe, alpha);
            let actual = keys[i].1.cframe;
            // Every key skipped since the anchor must still fit the line too.
            let fits = (anchor + 1..=i).all(|j| {
                let a = (keys[j].0 - t0) / (t1 - t0).max(f32::EPSILON);
                let g = k0.cframe.lerp(&k1.cframe, a);
                let k = keys[j].1.cframe;
                g.position.distance(k.position) <= tolerance
                    && g.rotation.angle_between(k.rotation) <= tolerance
            });
            if fits
                && guess.position.distance(actual.position) <= tolerance
                && keys[i].1.style == k0.style
            {
                keep[i] = false;
            } else {
                anchor = i;
            }
        }
        for (i, (time, _)) in keys.iter().enumerate() {
            if !keep[i] {
                clip.remove_pose(*time, &joint);
            }
        }
    }
}

#[cfg(test)]
mod tests;
