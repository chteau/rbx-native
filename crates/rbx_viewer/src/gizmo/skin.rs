//! Studio's "Next-Gen Draggers" beta: the same Move, Rotate and Scale
//! handles in a second skin, read off Studio 0.748's DraggerFramework
//! (`getFFlagNextGenDraggers`) and checked against screenshots of it.
//!
//! What changes with it on: the axis colours (Settings > Selection >
//! Dragger Visualization's swatches), handles two thirds the size, Move's
//! arrows on the positive ends only with a square per plane between them,
//! and Rotate's rings drawn on their near half inside a white view ring,
//! with a trackball inside them. The geometry the handles are grabbed by
//! changes with what is drawn, so this lives beside [`Handles`].

use glam::Vec3;

use super::{
    ring_crossing, Axis, Handles, HEAD_RADIUS, HEAD_START, MIN_DISTANCE, RING_PICK, RING_RADIUS,
    SHAFT_RADIUS,
};
use crate::pick::{self, Ray};
use crate::Pose;

/// Which look the draggers take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Skin {
    #[default]
    Classic,
    NextGen,
}

/// Studio sizes its handles `sin(fov) * depth * k`, `k` 0.05 classic and
/// 0.033 times DraggerScaleFactor (1 by default) next-gen. Measured on a
/// 4-stud cube 28.1 studs off (a 397 px high view): Move's arrows reach
/// 4.96 studs from the pivot and Rotate's rings are 3.87 studs round, so
/// `k` per handle is these over `sin(fov) * depth`.
const MOVE_REACH: f32 = 0.188;
const RING_REACH: f32 = 0.1467;
/// An orthographic view has no depth to size by: the classic arm, scaled as
/// Studio's two `k`s are.
const ORTHOGRAPHIC_SCALE: f32 = 0.033 / 0.05;
/// A hovered next-gen arrow grows its radius by this much and keeps its
/// length (classic grows the head instead, which this renderer lights).
pub(crate) const HOVER_THICKNESS: f32 = 1.3;
/// Where a plane square's centre sits along each of its two axes, and half
/// its side, in arm lengths: measured off Studio's screenshot of a 1-stud
/// cube (the squares' near corner level with the arrows' shaft start).
pub(crate) const PLANE_AT: f32 = 0.45;
pub(crate) const PLANE_HALF: f32 = 0.07;
/// The view ring's radius, in arm lengths: 4.36 studs against the axis
/// rings' 3.87 in the same screenshot.
pub(crate) const VIEW_RING_RADIUS: f32 = 1.13;
/// The view ring and view handles' colour, sRGB (230, 230, 230).
pub(crate) const VIEW_COLOR: [f32; 3] = [0.791, 0.791, 0.791];

impl Skin {
    /// The axis colour, linearized like [`Axis::color`]: next-gen's sRGB
    /// (237, 77, 59), (71, 184, 79) and (68, 127, 253).
    pub fn color(self, axis: Axis) -> [f32; 3] {
        match (self, axis) {
            (Skin::Classic, _) => axis.color(),
            (Skin::NextGen, Axis::X) => [0.847, 0.074, 0.044],
            (Skin::NextGen, Axis::Y) => [0.063, 0.479, 0.078],
            (Skin::NextGen, Axis::Z) => [0.058, 0.212, 0.982],
        }
    }

    /// How a Move arrow is built in this skin, in arm lengths. Next-gen's
    /// is Studio's MoveHandleView: a shaft of radius `0.1 s` and a head of
    /// radius `3 r` and length `L / 4`, `L` being `4 s` of a `4.7 s` reach.
    pub(crate) fn profile(self) -> Profile {
        match self {
            Skin::Classic => Profile {
                shaft: SHAFT_RADIUS,
                head: HEAD_RADIUS,
                neck: HEAD_START,
            },
            Skin::NextGen => Profile {
                shaft: 0.1 / 4.7,
                head: 0.3 / 4.7,
                neck: 1.0 - 1.0 / 4.7,
            },
        }
    }
}

/// A Move arrow's shape: its shaft's and head's radii and where the head
/// starts, in arm lengths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Profile {
    pub(crate) shaft: f32,
    pub(crate) head: f32,
    pub(crate) neck: f32,
}

impl Profile {
    /// Both radii times `by`, the length kept.
    pub(crate) fn thicker(self, by: f32) -> Profile {
        Profile {
            shaft: self.shaft * by,
            head: self.head * by,
            ..self
        }
    }
}

impl Handles {
    /// These handles in `skin`, seen from `pose`: next-gen sizes them as
    /// Studio does (`rings` for Rotate's, whose reach differs from Move's),
    /// and its rings and view ring depend on where the camera is.
    pub fn skinned(self, skin: Skin, pose: Pose, orthographic: bool, rings: bool) -> Self {
        let arm = match skin {
            Skin::Classic => self.arm,
            Skin::NextGen if orthographic => self.arm * ORTHOGRAPHIC_SCALE,
            Skin::NextGen => {
                let depth = (self.origin - pose.position)
                    .dot(pose.basis().0)
                    .max(MIN_DISTANCE);
                let reach = if rings { RING_REACH } else { MOVE_REACH };
                reach * pose.fov_degrees.to_radians().sin() * depth
            }
        };
        Handles {
            arm,
            skin,
            eye: pose.position,
            ..self
        }
    }

    pub fn skin(&self) -> Skin {
        self.skin
    }

    pub(crate) fn next_gen(&self) -> bool {
        self.skin == Skin::NextGen
    }

    /// Whether a point on a ring faces the camera: next-gen draws and grabs
    /// only that half.
    pub(crate) fn faces_eye(&self, point: Vec3) -> bool {
        !self.next_gen() || (point - self.origin).dot(self.eye - self.origin) >= 0.0
    }

    /// The square for the plane square to `axis`: its two in-plane axes and
    /// its centre.
    pub fn plane(&self, axis: Axis) -> (Vec3, Vec3, Vec3) {
        let (a, b) = (axis.next(), axis.next().next());
        let (a, b) = (self.direction(a), self.direction(b));
        (a, b, self.origin + (a + b) * PLANE_AT * self.arm)
    }

    /// Which plane square `ray` is on, by the axis square to it: next-gen's
    /// Move only.
    pub fn grab_plane(&self, ray: Ray) -> Option<Axis> {
        if !self.next_gen() {
            return None;
        }
        let half = PLANE_HALF * self.arm * 1.25;
        Axis::ALL
            .into_iter()
            .filter_map(|axis| {
                let (a, b, centre) = self.plane(axis);
                let hit = pick::ray_hits_plane(ray, centre, self.direction(axis))?;
                let offset = hit - centre;
                (offset.dot(a).abs() <= half && offset.dot(b).abs() <= half)
                    .then(|| (axis, (hit - ray.origin).length()))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(axis, _)| axis)
    }

    /// The view ring's frame, as [`Handles::ring_frame`] gives an axis
    /// ring's: square to the line from the camera.
    pub fn view_frame(&self) -> (Vec3, Vec3, Vec3) {
        let normal = (self.origin - self.eye).normalize_or(Vec3::NEG_Z);
        let across = normal
            .cross(Vec3::Y)
            .normalize_or(normal.any_orthonormal_vector());
        (normal, across.cross(normal), across)
    }

    /// Whether `ray` is on next-gen's view ring.
    pub fn grab_view_ring(&self, ray: Ray) -> bool {
        self.next_gen()
            && ring_crossing(self.origin, self.view_frame(), ray).is_some_and(|(_, out, _)| {
                (out - VIEW_RING_RADIUS * self.arm).abs() <= RING_PICK * self.arm
            })
    }

    /// The trackball's radius in studs: the axis rings'.
    pub fn trackball_radius(&self) -> f32 {
        RING_RADIUS * self.arm
    }

    /// Where `ray` meets next-gen's trackball, the sphere the rings lie on,
    /// as a direction from its centre: a drag there turns the part freely.
    pub fn grab_trackball(&self, ray: Ray) -> Option<Vec3> {
        let radius = self.trackball_radius();
        let (along, off) = ray.nearest(self.origin);
        (self.next_gen() && along > 0.0 && off < radius)
            .then(|| trackball_point(self.origin, radius, ray))
    }
}

/// Where `ray` meets the sphere of `radius` round `origin`, as a unit
/// direction from it, on the near side; a ray that misses lands on the
/// sphere's rim nearest to it, so a drag past the edge keeps turning.
pub fn trackball_point(origin: Vec3, radius: f32, ray: Ray) -> Vec3 {
    let (along, off) = ray.nearest(origin);
    let point = if off < radius {
        ray.at(along - (radius * radius - off * off).sqrt())
    } else {
        ray.at(along)
    };
    (point - origin).normalize_or(Vec3::Z)
}

#[cfg(test)]
#[path = "skin/tests.rs"]
mod tests;
