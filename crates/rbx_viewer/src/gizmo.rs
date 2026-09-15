//! The viewport's transform draggers: where each axis handle sits in the
//! world, which one a ray is pointing at, and how far along it a drag has
//! travelled.
//!
//! Pure geometry, no GPU and no DOM. Both halves of the editor read it: the
//! renderer builds the arrows' triangles from [`Handles`] (see
//! `renderer::gizmo`), and `rbxstudio`'s viewport hit-tests the very same
//! [`Handles`] against the cursor ray. One definition, so what you can grab is
//! exactly what you can see.
//!
//! Matches Studio's documented Move tool (`creator-docs`
//! `parts/index.md#transform-parts`): one arrow per axis, coloured red/green/
//! blue for X/Y/Z, drawn in world orientation or — with the local toggle on —
//! in the part's own frame.

use glam::{Mat3, Vec3};

use crate::pick::Ray;
use crate::Pose;

/// The arm length of one dragger as a fraction of the viewport's half-height,
/// so the gizmo keeps the same size on screen however far away the part is.
/// Chosen to read like Studio's: comfortably grabbable without burying a
/// small part inside its own handles.
const SCREEN_FRACTION: f32 = 0.16;
/// Where a dragger's shaft starts, in arm lengths. The gap around the origin
/// is what leaves the part itself clickable for a free cursor drag.
pub(crate) const SHAFT_START: f32 = 0.18;
/// Where the shaft ends and the arrowhead begins, in arm lengths.
pub(crate) const HEAD_START: f32 = 0.78;
/// The shaft's and the arrowhead's radii, in arm lengths.
pub(crate) const SHAFT_RADIUS: f32 = 0.018;
pub(crate) const HEAD_RADIUS: f32 = 0.06;
/// How far off a dragger's centre line a ray still counts as grabbing it, in
/// arm lengths. Wider than the arrowhead on purpose: a handle that can only
/// be grabbed by its exact silhouette is one the user misses repeatedly.
const PICK_RADIUS: f32 = 0.085;
/// A part sitting practically on top of the camera would otherwise scale its
/// gizmo to nothing; a perspective dragger never shrinks below the size it
/// has at this distance.
const MIN_DISTANCE: f32 = 1.0;

/// Which axis a dragger acts along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// The dragger's colour: Studio's red/green/blue for X/Y/Z, given here
    /// already linearized (from sRGB `(0.90, 0.16, 0.16)`, `(0.16, 0.78,
    /// 0.22)` and `(0.20, 0.36, 0.95)`) the way `scene::srgb_to_linear`
    /// linearizes every other colour in this renderer, so the HDR target's
    /// tonemap lands them back on those values rather than on washed-out
    /// ones.
    pub fn color(self) -> [f32; 3] {
        match self {
            Axis::X => [0.787, 0.022, 0.022],
            Axis::Y => [0.022, 0.649, 0.040],
            Axis::Z => [0.033, 0.106, 0.890],
        }
    }
}

/// What the viewport draws over the selection, and in which frame of
/// reference.
///
/// Only the Move draggers exist so far; Scale's handles and Rotate's rings add
/// a second field here rather than a second type when they land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gizmo {
    /// The part's own orientation rather than the world's — Studio's
    /// `Ctrl`/`Cmd`+`L` toggle.
    pub local: bool,
}

/// One part's draggers, placed in the world: where they meet, which way each
/// axis points, and how long an arm is in studs at the current camera
/// distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handles {
    origin: Vec3,
    basis: [Vec3; 3],
    arm: f32,
}

impl Handles {
    pub fn new(origin: Vec3, basis: [Vec3; 3], arm: f32) -> Self {
        Handles { origin, basis, arm }
    }

    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    /// The unit vector this axis's dragger points along.
    pub fn direction(&self, axis: Axis) -> Vec3 {
        self.basis[axis as usize]
    }

    /// One arm's length in studs.
    pub fn arm(&self) -> f32 {
        self.arm
    }

    /// Which dragger `ray` is pointing at, if any — the nearest one, so an
    /// axis pointing at the camera never loses to the one behind it.
    ///
    /// Both directions of each axis count: Studio's Move gizmo puts an arrow
    /// on each end, and grabbing either drags along the same line.
    pub fn grab(&self, ray: Ray) -> Option<Axis> {
        let reach = PICK_RADIUS * self.arm;
        Axis::ALL
            .into_iter()
            .filter_map(|axis| {
                let direction = self.direction(axis);
                let along = along_axis(self.origin, direction, ray)?;
                // Off the ends and into the gap around the origin, the nearest
                // point on the *dragger* is its endpoint, not the nearest point
                // on the infinite axis line.
                let sign = if along < 0.0 { -1.0 } else { 1.0 };
                let clamped = along.abs().clamp(SHAFT_START * self.arm, self.arm) * sign;
                let (distance, offset) = ray.nearest(self.origin + direction * clamped);
                (offset <= reach && distance > 0.0).then_some((axis, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(axis, _)| axis)
    }
}

/// How far along the line through `origin` in direction `axis` the point
/// closest to `ray` sits, in studs. `None` when the two are close enough to
/// parallel that the answer would be meaningless — which is exactly when a
/// drag along that axis has no usable screen direction either.
///
/// This is the whole of an axis drag: sample it once when the handle is
/// grabbed and again on every move, and the difference is how far the part
/// travelled.
pub fn along_axis(origin: Vec3, axis: Vec3, ray: Ray) -> Option<f32> {
    let between = origin - ray.origin;
    let projection = axis.dot(ray.direction);
    let denominator = 1.0 - projection * projection;
    if denominator < 1e-4 {
        return None;
    }
    Some((projection * between.dot(ray.direction) - between.dot(axis)) / denominator)
}

/// The world-space directions the three draggers point along: the world axes,
/// or — with the local toggle on — the part's own, taken from its rotation.
///
/// A degenerate rotation (a part scaled to nothing on an axis, so its basis
/// vector can't be normalized) falls back to the world axis rather than
/// producing a NaN dragger that can never be grabbed.
pub fn basis(rotation: Option<Mat3>) -> [Vec3; 3] {
    let Some(rotation) = rotation else {
        return [Vec3::X, Vec3::Y, Vec3::Z];
    };
    [
        rotation.x_axis.normalize_or(Vec3::X),
        rotation.y_axis.normalize_or(Vec3::Y),
        rotation.z_axis.normalize_or(Vec3::Z),
    ]
}

/// How long one dragger arm should be, in studs, for a gizmo at `origin` seen
/// from this pose — the conversion that keeps the handles a constant size on
/// screen.
///
/// Under a parallel projection there is no such thing as camera distance, so
/// the zoom level (`Pose::ortho_scale`, itself a half-height in studs) stands
/// in for it directly.
pub fn arm_length(origin: Vec3, pose: Pose, orthographic: bool) -> f32 {
    if orthographic {
        return pose.ortho_scale * SCREEN_FRACTION;
    }
    let distance = (origin - pose.position).length().max(MIN_DISTANCE);
    distance * (pose.fov_degrees * 0.5).to_radians().tan() * SCREEN_FRACTION
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
