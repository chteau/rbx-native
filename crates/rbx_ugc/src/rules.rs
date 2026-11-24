//! One function per rule family, each appending its findings.

mod basic;
mod cages;
mod degenerate;
mod distance;
mod fit;
mod size;
mod watertight;

pub(crate) use basic::{attachment, influences, joints, textures, triangles, unreadable};
pub use cages::cage_uv_mismatches;
pub(crate) use cages::cages;
pub(crate) use degenerate::degenerate;
pub use degenerate::degenerate_triangles;
pub(crate) use fit::fit;
pub use fit::{cage_distance_failures, cage_mesh_distances, mesh_outside_cage};
pub(crate) use size::size;
pub(crate) use watertight::watertight;
pub use watertight::{open_edges, weld, welded_triangles};

/// A share as Studio's messages print it: rounded to a tenth, written with
/// two decimals ("1.70%").
fn percent(share: f32) -> String {
    format!("{:.2}%", (share * 10.0).round() / 10.0)
}
