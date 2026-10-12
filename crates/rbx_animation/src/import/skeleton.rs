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

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::clip::{Clip, PoseKey};

pub use super::retarget::retarget;
use crate::frame::Frame;

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
    pub(crate) fn rest_world(&self) -> Vec<(Frame, f32)> {
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

    /// World frames (scale folded into positions) at `time`, every bone at
    /// its keyed local transform.
    pub(crate) fn world_at(&self, time: f32) -> Vec<Frame> {
        let mut out: Vec<(Frame, f32)> = Vec::with_capacity(self.bones.len());
        for (i, b) in self.bones.iter().enumerate() {
            let (parent, scale) = match b.parent {
                Some(p) if p < i => out[p],
                _ => (Frame::IDENTITY, 1.),
            };
            let local = self.local(i, time);
            let world = Frame::new(
                parent.position + parent.rotation * (local.position * scale),
                (parent.rotation * local.rotation).normalize(),
            );
            out.push((world, scale * b.scale));
        }
        out.into_iter().map(|(f, _)| f).collect()
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
    let (ca, cb) = (from.iter().sum::<Vec3>() / n, to.iter().sum::<Vec3>() / n);
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
    // The best rotation is N's eigenvector of largest eigenvalue. Shifted to
    // positive definite, repeated squaring makes that eigenvector dominate
    // every column (plain power iteration converges too slowly when the top
    // eigenvalues are close, as they are after the shift).
    let shift = n.to_cols_array().iter().map(|v| v.abs()).sum::<f32>();
    let mut m = n + Mat4::from_diagonal(Vec4::splat(shift));
    for _ in 0..40 {
        m = m * m;
        let biggest = m.to_cols_array().iter().fold(0f32, |a, v| a.max(v.abs()));
        if biggest <= 0. || !biggest.is_finite() {
            return None;
        }
        m *= 1. / biggest;
    }
    let q = [m.x_axis, m.y_axis, m.z_axis, m.w_axis]
        .into_iter()
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
        .normalize();
    // Horn's quaternion is (w, x, y, z).
    let rotation = Quat::from_xyzw(q.y, q.z, q.w, q.x).normalize();
    Some(Fit {
        rotation,
        scale: (spread_b / spread_a).sqrt(),
    })
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
