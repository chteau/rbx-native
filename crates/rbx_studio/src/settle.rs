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

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::{self, Meshes, Ray};
use rbx_viewer::Pose;

use crate::dragger::depth_scale;
use crate::dragger::free::{self, Landing};
use crate::dragger::surface::{surface_frame, SurfaceFrame};

/// One step of a cursor drag, as the view describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settle {
    /// The ray under the cursor now.
    pub(crate) cursor: Ray,
    /// Where the dragged point — the point the drag holds the selection by,
    /// fixed at the press — stands relative to the anchor part's centre.
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
}

/// Where a drag step landed, and on what.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settled {
    /// The anchor part's new centre.
    pub(crate) centre: Vec3,
    /// The face landed on, framed on its corner nearest the cursor.
    pub(crate) frame: SurfaceFrame,
    /// Where the cursor met it.
    pub(crate) hit: Vec3,
    pub(crate) landing: Landing,
    /// The grid the step was snapped to, which its ruler is ticked in.
    pub(crate) grid: f32,
}

/// Where the parts `dragged` (the anchor first) come to rest for this step,
/// or `None` when the cursor is over nothing and no step has landed yet —
/// the caller keeps its own flat-plane answer for that.
pub(crate) fn settled(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    dragged: &[Ref],
    settle: Settle,
) -> Option<Settled> {
    let anchor = pick::model_of(dom, *dragged.first()?)?.w_axis.truncate();
    let (frame, hit) = match target_under(dom, database, meshes, settle.cursor, dragged) {
        Some((model, hit)) => (surface_frame(model, hit)?, hit),
        None => {
            let frame = settle.last?;
            (
                frame,
                pick::ray_hits_plane(settle.cursor, frame.corner, frame.y)?,
            )
        }
    };
    let models = dragged.iter().filter_map(|&part| pick::model_of(dom, part));
    let bounds = free::bounds(&frame, models, anchor + settle.grabbed);
    let reach = settle
        .snap_to_parts
        .then(|| free::SOFT_SNAP_REACH * depth_scale(hit, settle.pose, settle.orthographic));
    let landing = free::land(&frame, hit, bounds, settle.grid, reach);
    Some(Settled {
        centre: landing.dragged(&frame) - settle.grabbed,
        frame,
        hit,
        landing,
        grid: settle.grid,
    })
}

/// The nearest drawn part along `ray` that is not being dragged, as the box
/// it is drawn in and where `ray` meets that box.
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
) -> Option<(Mat4, Vec3)> {
    let nearest = pick::parts_along(dom, database, meshes, ray)
        .into_iter()
        .find(|referent| !dragged.contains(referent))?;
    let model = pick::model_of(dom, nearest)?;
    Some((model, ray.at(pick::ray_hits_box(ray, model)?)))
}

#[cfg(test)]
#[path = "settle/tests.rs"]
mod tests;
