//! Outer cages for the cut body: Roblox's own template cages, bent onto the
//! fit, so layered clothing can wrap the rig.
//!
//! A body's `<Part>_OuterCage` has to keep the vertices and UVs of the
//! template cage Roblox ships for its reference body (creator-docs,
//! `avatar/character-bodies/specifications.md`, "Outer cages"): layered
//! clothing pairs its inner cage with the body's vertex for vertex. An
//! envelope of our own would import, but no garment would match it. So each
//! template part cage is moved, never remeshed: the template part is turned
//! onto the fitted bone and scaled to the cut part, the seams shared between
//! parts are welded again, and the result is pushed out of the body and
//! smoothed so it stays a loose, even shell.
//!
//! The template cages are not shipped here (they are Roblox's assets): the
//! caller downloads them and hands them in as [`CageTemplate`]s.

use glam::Vec3;

use crate::mesh::Mesh;
use crate::rig::Part;
use crate::split::Body;

/// One body part's template outer cage, as the reference body carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct CageTemplate {
    pub part: Part,
    /// The cage in the template part's frame (its `CageOrigin` already
    /// added), with its UVs. Normals may be empty.
    pub mesh: Mesh,
    /// The template part's `InitialSize`: the body part the cage was made
    /// around, centred on the part's origin.
    pub size: Vec3,
    /// The template part's `<Joint>RigAttachment`s, in its frame, by name:
    /// its own joint's and its children's, as [`crate::PartMesh::attachments`].
    pub attachments: Vec<(String, Vec3)>,
}

/// Why the cages could not be fitted.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CageError {
    #[error("no template cage for {0}")]
    MissingTemplate(Part),
    #[error("the template {part} has no {attachment}")]
    MissingAttachment { part: Part, attachment: String },
}

/// The outer cage of every part of `body`, in [`Part::ALL`] order, each
/// relative to its part's `centre` like [`crate::PartMesh::mesh`]: the
/// template's vertex order, UVs and triangles, with moved positions and
/// recomputed normals.
pub fn fit_cages(body: &Body, templates: &[CageTemplate]) -> Result<Vec<Mesh>, CageError> {
    let _ = (body, templates, Vec3::ZERO);
    todo!()
}
