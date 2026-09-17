//! The Scale tool's ball handles: where each one sits on the box the part
//! occupies, how big it is drawn, and which one a ray is pointing at.
//!
//! Split out from [`super`] because Scale's geometry has nothing in common
//! with the arms Move and Rotate share. An arm reaches a fixed distance *on
//! screen* out of the gizmo's pivot; a ball is pinned to the part's own
//! surface, so on a baseplate the handles stand a thousand studs out at the
//! plate's real edges rather than bunched around its middle.
//!
//! `creator-docs` describes exactly this shape for the engine's own equivalent
//! of the tool: `Enum.HandlesStyle.Resize` (the default) "renders
//! `Class.Handles` as sphere shapes for resizing an adornee along its face
//! axes", one per face named by `Class.Handles.Faces`.

use glam::{Mat3, Mat4, Vec3};

use crate::pick::Ray;
use crate::Pose;

use super::{arm_length, basis, Axis};

/// A ball's radius as a fraction of a dragger arm (see [`super::arm_length`]),
/// which is what keeps it a constant size on screen however far out on a large
/// part's surface it stands. Sized to read at the same weight as a Move
/// arrowhead (`super::HEAD_RADIUS`) rather than as a bead.
const BALL_RADIUS: f32 = 0.1;
/// How much wider than the drawn ball a ray still counts as grabbing it, in
/// ball radii. Wider than the silhouette on purpose, for the same reason the
/// arms' own pick radius is: a handle grabbable only by its exact outline is
/// one the user misses repeatedly.
const PICK: f32 = 1.4;
/// Half the unit cube's side. A part's model matrix already folds its `Size`
/// into its columns, so half a column reaches from the centre exactly to the
/// middle of that axis's face — the same ±0.5 `renderer::selection` draws the
/// selection outline's corners from, so a ball lands on the outline it is
/// grabbing.
const HALF: f32 = 0.5;

/// The Scale tool's handles: a ball on the middle of each of the six faces of
/// the box a part occupies, sized for the camera it is seen from.
///
/// Bound to the part's *own* axes whichever way the world/local toggle stands,
/// unlike [`super::Handles`]: `BasePart.Size` is expressed along those axes
/// and nothing else, so a ball on a world axis a turned part is not aligned to
/// would name a face that does not exist and a resize that cannot be written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Faces {
    centre: Vec3,
    /// The part's own axes, normalized — [`super::basis`]'s answer for its
    /// rotation, so a part flattened to nothing on one axis still has a
    /// grabbable handle there rather than a NaN one.
    basis: [Vec3; 3],
    /// Half the part's `Size`, which is how far each face stands off the
    /// centre along its own axis.
    half: Vec3,
    pose: Pose,
    orthographic: bool,
}

impl Faces {
    /// `model` is the part's placement, `Size` already folded into its columns
    /// the way `pick::model_of` builds it.
    pub fn new(model: Mat4, pose: Pose, orthographic: bool) -> Self {
        let linear = Mat3::from_mat4(model);
        Faces {
            centre: model.w_axis.truncate(),
            basis: basis(Some(linear)),
            half: HALF
                * Vec3::new(
                    linear.x_axis.length(),
                    linear.y_axis.length(),
                    linear.z_axis.length(),
                ),
            pose,
            orthographic,
        }
    }

    pub fn centre(&self) -> Vec3 {
        self.centre
    }

    /// The outward unit normal of this axis's `+` face — the part's own axis,
    /// which is the only direction its `Size` can grow along.
    /// How long the box is along one of its own axes — what a pull on that
    /// axis's ball is measured against when a group scales in proportion.
    pub fn extent(&self, axis: Axis) -> f32 {
        2.0 * self.half[axis as usize]
    }

    pub fn direction(&self, axis: Axis) -> Vec3 {
        self.basis[axis as usize]
    }

    /// Where one ball stands: the middle of the face `sign` picks out on
    /// `axis`, `+1` for the one the axis points through and `-1` for the one
    /// opposite it.
    pub fn handle(&self, axis: Axis, sign: f32) -> Vec3 {
        self.centre + self.direction(axis) * (self.half[axis as usize] * sign)
    }

    /// How big that ball is drawn, in studs — taken at the ball's own distance
    /// from the eye rather than at the part's centre's. On a part the size of
    /// a baseplate the near and far handles are a thousand studs apart, and
    /// one radius for the whole box would leave the far ones specks and the
    /// near ones boulders.
    pub fn radius(&self, axis: Axis, sign: f32) -> f32 {
        BALL_RADIUS * arm_length(self.handle(axis, sign), self.pose, self.orthographic)
    }

    /// Every face, in a fixed order: each axis's `+` side, then its `-` side.
    pub fn all(&self) -> impl Iterator<Item = (Axis, f32)> {
        Axis::ALL
            .into_iter()
            .flat_map(|axis| [(axis, 1.0f32), (axis, -1.0f32)])
    }

    /// Which ball `ray` is pointing at and which face of the part it sits on —
    /// the nearest one, so a handle on the near face never loses to the one
    /// behind the part.
    ///
    /// The sign is what tells a Scale drag which face the cursor has hold of,
    /// and so which way the part grows as the cursor pulls away from it.
    pub fn grab(&self, ray: Ray) -> Option<(Axis, f32)> {
        self.all()
            .filter_map(|(axis, sign)| {
                let (distance, offset) = ray.nearest(self.handle(axis, sign));
                let reach = PICK * self.radius(axis, sign);
                (offset <= reach && distance > 0.0).then_some((axis, sign, distance))
            })
            .min_by(|(.., a), (.., b)| a.total_cmp(b))
            .map(|(axis, sign, _)| (axis, sign))
    }
}

#[cfg(test)]
#[path = "faces/tests.rs"]
mod tests;
