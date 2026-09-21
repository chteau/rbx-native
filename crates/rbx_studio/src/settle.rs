//! Resting a cursor-dragged part on whatever the cursor is over.
//!
//! Studio's cursor drag does not slide a part across a fixed plane: it lands
//! the part on the surface under the cursor, so dragging across a scene
//! carries it up onto a platform and back down again, the way an object dragged
//! across a tabletop rides over whatever lies on it. creator-docs
//! (`parts/index.md#transform-parts`) describes the rest of that gesture as
//! soft-snapping "to surfaces and edges of nearby parts"; the surface half is
//! what lives here, the edge half does not exist yet.
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

/// One step of a cursor drag, as the view describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settle {
    /// The ray under the cursor now.
    pub(crate) cursor: Ray,
    /// The ray the part was grabbed along, starting at the point on the part
    /// it was grabbed by. Fixed for the whole gesture: it is what "the same
    /// place relative to the cursor" is measured from.
    pub(crate) grab: Ray,
    /// Where the part's centre stood when it was grabbed.
    pub(crate) centre: Vec3,
}

/// A point on some part's face, and which way that face looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Surface {
    pub(crate) point: Vec3,
    pub(crate) normal: Vec3,
}

/// Where the part `referent` comes to rest for this step, or `None` when the
/// cursor is over nothing but sky — the caller keeps its own flat-plane answer
/// for that.
pub(crate) fn settled(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    referent: Ref,
    settle: Settle,
) -> Option<Vec3> {
    let model = pick::model_of(dom, referent)?;
    let surface = surface_under(dom, database, meshes, settle.cursor, referent)?;
    Some(rest_on(settle, model, surface))
}

/// The nearest drawn face along `ray`, leaving `exclude` out of the search.
///
/// The exclusion is the part being dragged: it is under the cursor by
/// definition, so a search that could see it would find it first every time
/// and rest the part on top of itself, one height higher per frame.
pub(crate) fn surface_under(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    ray: Ray,
    exclude: Ref,
) -> Option<Surface> {
    part_under(dom, database, meshes, ray, Some(exclude)).map(|(_, surface)| surface)
}

/// [`surface_under`], saying which part the face belongs to, and with the
/// exclusion optional: the Sun tool aims at whatever is nearest, and leaves
/// out only the part whose shadow it is placing.
pub(crate) fn part_under(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    ray: Ray,
    exclude: Option<Ref>,
) -> Option<(Ref, Surface)> {
    let nearest = pick::parts_along(dom, database, meshes, ray)
        .into_iter()
        .find(|&referent| Some(referent) != exclude)?;
    Some((nearest, face_hit(ray, pick::model_of(dom, nearest)?)?))
}

/// The face of the box drawn with `model` that `ray` enters, or `None` when
/// the ray misses it.
pub(crate) fn face_hit(ray: Ray, model: Mat4) -> Option<Surface> {
    let distance = pick::ray_hits_box(ray, model)?;
    let point = ray.at(distance);

    // On the unit cube's surface exactly one coordinate stands at ±0.5, so the
    // face is whichever axis the hit sits furthest along. A ray starting
    // inside the box hits at distance zero and lands on no face at all; the
    // nearest one is as good an answer as any there.
    let local = model.inverse().transform_point3(point);
    let axis = (0..3).max_by(|&a, &b| local[a].abs().total_cmp(&local[b].abs()))?;
    // Rotation and scale only, so a face normal is the box's own axis — the
    // inverse-transpose a sheared matrix would need never comes up.
    let normal = model.col(axis).truncate().normalize() * local[axis].signum();
    Some(Surface { point, normal })
}

/// Where the centre of the part drawn with `model` stands once it rests on
/// `surface`, keeping the grab where it was relative to the cursor.
///
/// The part's face that meets the surface is the one furthest back along the
/// surface's normal; the grab ray is followed on to that face's plane and the
/// point it meets there is what the cursor's hit point stands in for. That
/// choice, rather than the grab point itself, is what keeps a part that
/// already rests on a surface exactly still until the cursor actually moves:
/// followed on past the part, the grab ray meets the surface at that very
/// point.
pub(crate) fn rest_on(settle: Settle, model: Mat4, surface: Surface) -> Vec3 {
    let touching = settle.centre - surface.normal * reach(model, surface.normal);
    // A grab ray running along the touching plane (a part grabbed from above
    // and dragged onto a wall) meets it nowhere useful; the grab point dropped
    // straight onto the plane is the answer with no direction to argue about.
    let anchor = pick::ray_hits_plane(settle.grab, touching, surface.normal).unwrap_or_else(|| {
        let grabbed = settle.grab.origin;
        grabbed - surface.normal * (grabbed - touching).dot(surface.normal)
    });
    surface.point + (settle.centre - anchor)
}

/// How far the box drawn with `model` extends from its centre along the unit
/// `direction`: half its size on each axis, each turned onto that direction.
pub(crate) fn reach(model: Mat4, direction: Vec3) -> f32 {
    0.5 * (0..3)
        .map(|axis| model.col(axis).truncate().dot(direction).abs())
        .sum::<f32>()
}

#[cfg(test)]
#[path = "settle/tests.rs"]
mod tests;
