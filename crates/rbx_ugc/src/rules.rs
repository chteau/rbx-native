//! One function per rule family, each appending its findings.

mod basic;
mod cages;
mod size;
mod watertight;

pub(crate) use basic::{attachment, influences, joints, textures, triangles, unreadable};
pub(crate) use cages::cages;
pub(crate) use size::size;
pub(crate) use watertight::watertight;
