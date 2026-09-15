//! The viewport's transform draggers: where each axis handle sits in the
//! world, which one a ray is pointing at, and how far along it — or how far
//! around it — a drag has travelled.
//!
//! Pure geometry, no GPU and no DOM. Both halves of the editor read it: the
//! renderer builds the arrows, handles and rings' triangles from [`Handles`]
//! (see `renderer::gizmo`), and `rbxstudio`'s viewport hit-tests the very same
//! [`Handles`] against the cursor ray. One definition, so what you can grab is
//! exactly what you can see.
//!
//! Matches Studio's documented transform tools (`creator-docs`
//! `parts/index.md#transform-parts`): Move's arrow per axis, Scale's handle per
//! axis, Rotate's ring per axis, coloured red/green/blue for X/Y/Z and drawn in
//! world orientation or — with the local toggle on — in the part's own frame.

use glam::{Mat3, Vec3};

use crate::pick::Ray;
use crate::Pose;

/// The arm length of one dragger as a fraction of the viewport's half-height,
/// so the gizmo keeps the same size on screen however far away the part is.
/// Chosen to read like Studio's: comfortably grabbable without burying a
/// small part inside its own handles.
const SCREEN_FRACTION: f32 = 0.2;
/// Where a dragger's shaft starts, in arm lengths. The gap around the origin
/// is what leaves the part itself clickable for a free cursor drag.
pub(crate) const SHAFT_START: f32 = 0.24;
/// Where the shaft ends and the arrowhead begins, in arm lengths.
pub(crate) const HEAD_START: f32 = 0.72;
/// The shaft's and the arrowhead's radii, in arm lengths.
pub(crate) const SHAFT_RADIUS: f32 = 0.03;
pub(crate) const HEAD_RADIUS: f32 = 0.105;
/// Half the edge of a Scale handle's block, in arm lengths. Studio puts a
/// small block on the end of each arm where Move puts an arrowhead; this is
/// sized to read as the same weight as that head rather than as a bead.
pub(crate) const HANDLE_RADIUS: f32 = 0.085;
/// How far off a dragger's centre line a ray still counts as grabbing it, in
/// arm lengths. Wider than the arrowhead on purpose: a handle that can only
/// be grabbed by its exact silhouette is one the user misses repeatedly.
const PICK_RADIUS: f32 = 0.15;
/// A rotation ring's radius, in arm lengths — the same reach a Move arrow has,
/// so switching tools does not change how far out the user has to aim.
pub(crate) const RING_RADIUS: f32 = 1.0;
/// Half the thickness of a ring's drawn tube, in arm lengths.
pub(crate) const RING_THICKNESS: f32 = 0.024;
/// How far off a ring's own circle a ray still counts as grabbing it, in arm
/// lengths — wider than the tube, for the same reason [`PICK_RADIUS`] is.
const RING_PICK: f32 = 0.09;
/// Below this, a ray runs so nearly along a ring's plane that the crossing it
/// would report is numerical noise. Deliberately tiny rather than a
/// comfortable margin: the radius test alongside it already rejects a crossing
/// that landed nowhere near the circle, so this only has to keep the division
/// itself meaningful.
const RING_EDGE_ON: f32 = 1e-6;
/// A part sitting practically on top of the camera would otherwise scale its
/// gizmo to nothing; a perspective dragger never shrinks below the size it
/// has at this distance.
const MIN_DISTANCE: f32 = 1.0;

/// Which axis a dragger acts along, or — for Rotate — turns about.
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

    /// The next axis round the cycle X → Y → Z → X, which is where a ring
    /// takes its own zero angle from (see [`Handles::ring_frame`]).
    fn next(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::Z,
            Axis::Z => Axis::X,
        }
    }
}

/// Which transform tool's handles the viewport is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    /// An arrow along each axis; dragging one slides the part along it.
    #[default]
    Move,
    /// A block on the end of each arm; dragging one resizes the part along
    /// that axis.
    Scale,
    /// A ring around each axis; dragging one turns the part about it.
    Rotate,
}

/// What the viewport draws over the selection, and in which frame of
/// reference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gizmo {
    pub kind: Kind,
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

    /// Which dragger `ray` is pointing at and which end of it — the nearest
    /// one, so an axis pointing at the camera never loses to the one behind
    /// it. The sign is `+1` on the arm running along the axis and `-1` on the
    /// one opposite it, which is what tells Scale which face of the part the
    /// cursor has hold of.
    ///
    /// Both directions of each axis count: Studio's Move gizmo puts an arrow
    /// on each end, and grabbing either drags along the same line.
    pub fn grab_arm(&self, ray: Ray) -> Option<(Axis, f32)> {
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
                (offset <= reach && distance > 0.0).then_some((axis, sign, distance))
            })
            .min_by(|(.., a), (.., b)| a.total_cmp(b))
            .map(|(axis, sign, _)| (axis, sign))
    }

    /// Which dragger `ray` is pointing at, if any — [`Handles::grab_arm`]
    /// without the end it was grabbed by, which is all the Move tool needs.
    pub fn grab(&self, ray: Ray) -> Option<Axis> {
        self.grab_arm(ray).map(|(axis, _)| axis)
    }

    /// The frame one rotation ring lives in: the axis it turns about, then two
    /// perpendicular directions spanning the ring's own plane, the first of
    /// them standing at the ring's zero angle.
    ///
    /// Built from the gizmo's own basis rather than from an arbitrary
    /// perpendicular so that turning from the first towards the second is a
    /// *positive* rotation about the axis by the right-hand rule — which is
    /// what lets the angle this measures be handed straight to
    /// `Mat3::from_axis_angle`.
    pub fn ring_frame(&self, axis: Axis) -> (Vec3, Vec3, Vec3) {
        let normal = self.direction(axis);
        let reference = self.direction(axis.next());
        // A degenerate basis (see [`basis`]) can leave those two parallel; any
        // perpendicular will do then, since there is no part frame left to
        // agree with anyway.
        let across = normal
            .cross(reference)
            .normalize_or(normal.any_orthonormal_vector());
        (normal, across.cross(normal), across)
    }

    /// Which rotation ring `ray` is pointing at, if any — the nearest, so the
    /// ring in front of the part never loses to the one crossing behind it.
    pub fn grab_ring(&self, ray: Ray) -> Option<Axis> {
        let reach = RING_PICK * self.arm;
        let radius = RING_RADIUS * self.arm;
        Axis::ALL
            .into_iter()
            .filter_map(|axis| {
                let (_, out, distance) = ring_crossing(self.origin, self.ring_frame(axis), ray)?;
                ((out - radius).abs() <= reach).then_some((axis, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(axis, _)| axis)
    }
}

/// Where `ray` crosses the plane through `origin` that a ring frame spans (see
/// [`Handles::ring_frame`]): the angle round the ring, how far out from the
/// centre the crossing landed, and how far along the ray it is. `None` when
/// the ray runs along that plane, where every angle is equally close to being
/// the answer.
///
/// The angle is the whole of a rotate drag: sample it once when the ring is
/// grabbed and again on every move, and the steps between those samples (see
/// [`angle_step`]) are how far the part has turned. A drag measures against
/// the frame it *started* in rather than against a live [`Handles`] — in local
/// orientation the handles turn with the part as it goes, and measuring
/// against those would cancel out the very rotation being applied.
pub fn ring_crossing(
    origin: Vec3,
    (normal, zero, quarter): (Vec3, Vec3, Vec3),
    ray: Ray,
) -> Option<(f32, f32, f32)> {
    let slope = ray.direction.dot(normal);
    if slope.abs() < RING_EDGE_ON {
        return None;
    }
    let distance = (origin - ray.origin).dot(normal) / slope;
    if distance <= 0.0 {
        return None;
    }

    let offset = ray.at(distance) - origin;
    let (x, y) = (offset.dot(zero), offset.dot(quarter));
    Some((y.atan2(x), x.hypot(y), distance))
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

/// The shortest way round from one ring angle to another, in radians.
///
/// A ring angle is only defined up to a full turn, so a drag that walks across
/// the seam reads as nearly a whole turn *backwards* unless each step is taken
/// the short way. Measuring every mouse move against the previous one and
/// summing these is what lets a drag pass 180° and keep going.
pub fn angle_step(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (to - from + PI).rem_euclid(TAU) - PI
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

/// A quarter turn about `axis`, the rotation `T` and `R` apply to a part
/// being cursor-dragged (`creator-docs` `parts/index.md#transform-parts`:
/// "`T` and `R` can be used to quickly rotate the part in 90&deg; increments
/// around the point you picked it up by").
///
/// Positive by the right-hand rule, which is what makes a turn about the
/// camera's right vector tilt the part's top *towards* the camera rather than
/// away from it.
pub fn quarter_turn(axis: Vec3) -> Mat3 {
    Mat3::from_axis_angle(axis.normalize_or(Vec3::Y), std::f32::consts::FRAC_PI_2)
}

/// A placement carried through `turn` about `pivot`: its linear part (a
/// rotation, possibly still carrying a part's `Size` in its column lengths)
/// and where it stands.
///
/// Shared by the two halves of a `T`/`R` turn — the viewport rotates the
/// matrix its draggers are drawn from, the editor rotates the `CFrame` it
/// writes into the DOM — so the handles cannot end up describing a different
/// rotation than the one that was saved.
pub fn turned(linear: Mat3, position: Vec3, pivot: Vec3, turn: Mat3) -> (Mat3, Vec3) {
    (turn * linear, pivot + turn * (position - pivot))
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
