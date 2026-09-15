//! `Beam` support: reading beams out of a DOM and resolving their `Attachment`
//! endpoints into world space ([`plan`]), independently of any GPU state.
//!
//! [`Scene::beams`](super::Scene::beams) is the only door in; the renderer
//! builds the ribbon geometry every frame from these static definitions,
//! since only it knows the eye position `FaceCamera` needs — see
//! `renderer::beam`.

mod attachment;
mod curve;
mod instance;

/// `scene::trail` resolves `Attachment0`/`Attachment1` the identical way and
/// reuses this rather than rebuilding the same parent walk.
pub(crate) use attachment::{world_cframe, ParentMap};
#[cfg(test)]
pub(crate) use curve::Curve;
pub(crate) use instance::{plan, Beam, TextureMode};
