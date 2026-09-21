//! Landing a cursor-dragged selection on whatever the cursor is over.
//!
//! Studio's cursor drag does not slide a part across a fixed plane: it lands
//! the part on the face under the cursor, so dragging across a scene carries
//! it up onto a platform and back down again, the way an object dragged
//! across a tabletop rides over whatever lies on it. Where exactly it lands on
//! that face — the grid measured from the face's nearest corner, and the
//! face's own edges and centre lines it aligns with — is Studio's
//! `DragHelper`, in `crate::dragger::free`.
//!
//! The split across the editor mirrors `workspace_view::gizmo`: the view
//! knows the cursor and the grab but has no DOM, so it describes the gesture
//! as a [`Settle`] and `Shell`, which has the DOM, answers it with
//! [`settled`]. Everything in between is pure geometry, so it can be tested
//! without a window or a GPU.
//!
//! Every answer is a function of the cursor alone — nothing here feeds the
//! part's previous position back in — which is what keeps a part from
//! oscillating between two surfaces when the cursor holds still on the edge
//! between them.

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::{self, Meshes, PartSurface, Ray};
use rbx_viewer::{gizmo, Pose};

use crate::dragger::depth_scale;
use crate::dragger::free::{self, Landing};
use crate::dragger::surface::{SurfaceFrame, TargetKind};
use crate::dragger::{target, tilt};

/// One step of a cursor drag, as the view describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settle {
    /// The ray under the cursor now.
    pub(crate) cursor: Ray,
    /// Where the dragged point — the point the drag holds the selection by,
    /// fixed at the press — stood relative to the anchor part's centre then.
    pub(crate) grabbed: Vec3,
    /// The grid in force for this step (`0.0` for none: snapping off, or
    /// `Shift` held).
    pub(crate) grid: f32,
    /// Snap to Parts on, and `Shift` up.
    pub(crate) snap_to_parts: bool,
    /// The camera the step was aimed with: a soft snap reaches a fixed
    /// distance on screen, not in studs.
    pub(crate) pose: Pose,
    pub(crate) orthographic: bool,
    /// The face the previous step landed on, which a cursor over nothing
    /// keeps landing on, in its plane.
    pub(crate) last: Option<SurfaceFrame>,
    /// Align Dragged Objects on and `Alt` up: the selection turns to lie on
    /// the face it lands on (see `crate::dragger::tilt`).
    pub(crate) align: bool,
    /// The quarter turns `R` and `T` have added this drag.
    pub(crate) tilt: Mat3,
    /// The last of them while it eases in: the tilt it turns from, and how
    /// far it has eased (see `crate::dragger::tilt::eased`).
    pub(crate) turning: Option<(Mat3, f32)>,
}

/// Where a drag step landed, and on what.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settled {
    /// The rigid move from where the parts stood at the grab to where this
    /// step rests them: every part's grab-time placement carried by it.
    pub(crate) carry: Mat4,
    /// The face landed on, framed on its corner nearest the cursor.
    pub(crate) frame: SurfaceFrame,
    /// Where the cursor met it.
    pub(crate) hit: Vec3,
    pub(crate) landing: Landing,
    /// The grid the step was snapped to, which its ruler is ticked in.
    pub(crate) grid: f32,
}

/// Where the parts `held` — each where it stood at the grab, the anchor
/// first — come to rest for this step, or `None` when the cursor is over
/// nothing and no step has landed yet: the caller keeps its own flat-plane
/// answer for that.
///
/// Studio's `getDragTargetNew`: the selection's box, in the anchor's own
/// frame at the grab, is turned onto the target's frame, lifted until its
/// underside rests on the face, and hung from the dragged point's landing.
pub(crate) fn settled(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    held: &[(Ref, Mat4)],
    settle: Settle,
) -> Option<Settled> {
    let &(_, anchor) = held.first()?;
    let dragged: Vec<Ref> = held.iter().map(|&(referent, _)| referent).collect();
    let found = target_under(dom, database, meshes, settle.cursor, &dragged)
        .and_then(|part| target::under(&part, settle.cursor, settle.grid));
    let (frame, hit) = match found {
        Some(found) => found,
        None => {
            // Studio's `Nothing`: the last frame as it stood, measured on
            // its grid but aligned with nothing.
            let frame = SurfaceFrame {
                kind: TargetKind::Nothing,
                size: Vec2::ZERO,
                part: None,
                ..settle.last?
            };
            (
                frame,
                pick::ray_hits_plane(settle.cursor, frame.corner, frame.y)?,
            )
        }
    };
    // The anchor's placement at the grab is the selection's basis, and the
    // dragged point and the selection's box are measured in its frame.
    let [x, y, z] = gizmo::basis(Some(Mat3::from_mat4(anchor)));
    let basis = Mat3::from_cols(x, y, z);
    let origin = anchor.w_axis.truncate();
    let point = basis.transpose() * settle.grabbed;
    let selection = free::selection_box(basis, origin, held.iter().map(|&(_, model)| model));
    let lying = tilt::in_frame(&frame, basis, settle.align);
    let reach = settle
        .snap_to_parts
        .then(|| free::SOFT_SNAP_REACH * depth_scale(hit, settle.pose, settle.orthographic));
    let land = |tilt: Mat3| {
        let bounds = free::bounds(lying * tilt, selection, point);
        free::land(&frame, hit, bounds, settle.grid, reach)
    };
    let landing = land(settle.tilt);
    // Turned as the box was, about the dragged point, which lands where the
    // landing says — or, while a turn eases in, part way from where it
    // stood before the turn, as Studio's tween lerps the two.
    let (turn, dragged) = match settle.turning {
        Some((from, eased)) => {
            let before = land(from).dragged(&frame);
            let between = Quat::from_mat3(&from).slerp(Quat::from_mat3(&settle.tilt), eased);
            (
                lying * Mat3::from_quat(between),
                before.lerp(landing.dragged(&frame), eased),
            )
        }
        None => (lying * settle.tilt, landing.dragged(&frame)),
    };
    let rotation = tilt::frame_rotation(&frame) * turn * basis.transpose();
    let translation = dragged - rotation * (origin + settle.grabbed);
    Some(Settled {
        carry: Mat4::from_cols(
            rotation.x_axis.extend(0.0),
            rotation.y_axis.extend(0.0),
            rotation.z_axis.extend(0.0),
            translation.extend(1.0),
        ),
        frame,
        hit,
        landing,
        grid: settle.grid,
    })
}

/// The surface of the nearest drawn part along `ray` that is not being
/// dragged.
///
/// The dragged parts are left out: they are under the cursor by definition,
/// so a search that could see them would find them first every time and rest
/// the selection on top of itself, one height higher per frame.
pub(crate) fn target_under(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    ray: Ray,
    dragged: &[Ref],
) -> Option<PartSurface> {
    let nearest = pick::parts_along(dom, database, meshes, ray)
        .into_iter()
        .find(|referent| !dragged.contains(referent))?;
    PartSurface::read(dom, database, meshes, nearest)
}

#[cfg(test)]
#[path = "settle/tests.rs"]
mod tests;
