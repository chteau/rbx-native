//! The viewport's transform draggers: where each axis handle sits in the
//! world, which one a ray is pointing at, and how far along it — or how far
//! around it — a drag has travelled.
//!
//! Pure geometry, no GPU and no DOM. Both halves of the editor read it: the
//! renderer builds the arrows, balls and rings' triangles from [`Handles`] and
//! [`Faces`] (see `renderer::gizmo`), and `rbxstudio`'s viewport hit-tests the
//! very same two against the cursor ray. One definition, so what you can grab
//! is exactly what you can see.
//!
//! Matches Studio's documented transform tools (`creator-docs`
//! `parts/index.md#transform-parts`): Move's arrow per axis and Rotate's ring
//! per axis, both reaching a constant distance on screen out of the
//! selection's pivot ([`Handles`]), and Scale's ball on each face of the
//! part's own box ([`Faces`]). Coloured red/green/blue for X/Y/Z, and drawn in
//! world orientation or — with the local toggle on — in the part's own frame.

use glam::{Mat3, Mat4, Vec3};

use crate::pick::Ray;
use crate::Pose;

mod faces;

pub use faces::Faces;

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
    /// A ball on the middle of each face of the part's own box; dragging one
    /// resizes the part along that face's axis.
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

/// The Move and Rotate tools' draggers, placed in the world: where they meet,
/// which way each axis points, and how long an arm is in studs at the current
/// camera distance.
///
/// Scale's handles are [`Faces`] instead: they are pinned to the part's own
/// surface rather than reaching a fixed distance on screen out of the pivot.
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
    /// one opposite it.
    ///
    /// Both directions of each axis count: Studio's Move gizmo puts an arrow
    /// on each end, and grabbing either drags along the same line.
    fn grab_arm(&self, ray: Ray) -> Option<(Axis, f32)> {
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

/// Where one gizmo goes for a whole selection: the centre of the world-axis
/// -aligned box that contains every one of `models`, the oriented boxes its
/// parts occupy. `None` for an empty selection.
///
/// The centre of the *bounds*, not the mean of the parts' own centres — those
/// differ as soon as the selection is lopsided (three small parts at one end
/// and one large at the other), and the bounds are what the user sees the
/// selection occupying. `creator-docs` never states where the gizmo sits for a
/// multi-object selection, but it is explicit that this is what Studio means
/// by the centre of an aggregate: the pivot tool's **Reset** "moves the pivot
/// point to the **center** of an object or model's bounding box"
/// (`studio/pivot-tools.md`).
///
/// One part is the same answer as before — its own bounding box is centred on
/// it — so this needs no special case for a single selection.
pub fn centre_of(models: impl IntoIterator<Item = Mat4>) -> Option<Vec3> {
    bounds_of(models).map(|(min, max)| (min + max) * 0.5)
}

/// The world-axis-aligned box containing every one of `models`, as its minimum
/// and maximum corner. `None` for an empty selection.
///
/// Split out of [`centre_of`] because a selected `Model` needs the whole box
/// and not just its middle: the outline drawn around a container is exactly
/// this extent, and deriving it a second time somewhere else is how the box
/// the user sees and the point the gizmo stands on start to disagree.
/// The box the Scale tool's handles stand on: a single part's own oriented
/// box, or — for more than one — the world-axis-aligned box round all of
/// them, as the `Mat4` `part_model` would give a box-shaped part of that
/// size at that centre. Shared by the renderer and the editor's hit test for
/// the same reason [`centre_of`] is: two derivations of "where the handles
/// are" are two things that can disagree.
///
/// `creator-docs` (`parts/models.md#transform-models`): "a model transforms
/// based on the center of its bounding box" — and the bounding box it means
/// is the world-aligned one `bounds_of` computes.
pub fn scale_box(models: impl IntoIterator<Item = Mat4>) -> Option<Mat4> {
    let models: Vec<Mat4> = models.into_iter().collect();
    match models.as_slice() {
        [] => None,
        [only] => Some(*only),
        many => {
            let (min, max) = bounds_of(many.iter().copied())?;
            Some(Mat4::from_translation((min + max) * 0.5) * Mat4::from_scale(max - min))
        }
    }
}

pub fn bounds_of(models: impl IntoIterator<Item = Mat4>) -> Option<(Vec3, Vec3)> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for model in models {
        let centre = model.w_axis.truncate();
        // A box turned off the world axes still has to be contained by them:
        // each world-axis half-extent is the sum of the absolute projections
        // of the three (already `Size`-scaled) columns onto that axis.
        let half = 0.5
            * Vec3::new(
                model.x_axis.x.abs() + model.y_axis.x.abs() + model.z_axis.x.abs(),
                model.x_axis.y.abs() + model.y_axis.y.abs() + model.z_axis.y.abs(),
                model.x_axis.z.abs() + model.y_axis.z.abs() + model.z_axis.z.abs(),
            );
        bounds = Some(match bounds {
            None => (centre - half, centre + half),
            Some((min, max)) => (min.min(centre - half), max.max(centre + half)),
        });
    }
    bounds
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

/// One frame's gizmo, as the geometry it is drawn and grabbed from.
///
/// Move and Rotate share [`Handles`]; Scale's [`Faces`] are a different shape
/// entirely, so there is no one type both halves of the editor can pass around
/// — this is it, and it is what keeps the renderer and the hit-test from being
/// handed different geometry for the same tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Move(Handles),
    Scale(Faces),
    Rotate(Handles),
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
