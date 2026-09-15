//! `Trail` support: reading trails out of a DOM ([`instance`]), recording the
//! segment history their two attachments trace over time ([`history`]), and
//! sampling their appearance along that history ([`ribbon`]) — all of it pure
//! data, independently of any GPU state.
//!
//! [`Scene::trails`](super::Scene::trails) is the only door in. Unlike a
//! `Beam`'s curve, a trail's shape is not static: it is built frame by frame
//! from wherever its attachments have been, so the renderer owns the running
//! [`history::Recorder`] per trail — see `renderer::trail`.
//!
//! This viewer never moves an `Attachment` between frames (no script, no
//! physics — see `crate::lighting`'s and `crate::scene::beam`'s identical
//! note), so in practice a `Trail` here only ever records a single sample and
//! never has the two it needs to draw a segment. That is a correct no-op, not
//! a gap: every type below is exercised by tests that replay a synthetic
//! motion path through the same code a future scripted scene would use.

mod history;
mod instance;
mod ribbon;

pub(crate) use history::Recorder;
pub(crate) use instance::{plan, Trail};
pub(crate) use ribbon::segments;
