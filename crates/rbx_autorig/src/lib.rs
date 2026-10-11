//! Turns an unrigged humanoid mesh into an R15 body: a fitted skeleton (15
//! joint positions) and 15 capped, watertight body-part meshes with their
//! rig attachments, ready to be uploaded and assembled into a rig.
//!
//! The pipeline, and the order a caller drives it in:
//!
//! 1. [`read`] a `.gltf`/`.glb`, `.obj` or binary `.fbx` into one [`Mesh`].
//! 2. [`fit`] the skeleton: detect up and front, scale to a height in studs,
//!    voxelise, find the five extremities (head, hands, feet) geodesically and
//!    place the joints from cross-section profiles and Mannequin proportions.
//!    The [`Fit`] is plain data meant to be corrected by hand; [`mirror`]
//!    makes the markers symmetric again after a drag.
//! 3. [`split`] the mesh into the 15 parts with exact plane cuts at the
//!    joints and caps on every opening.
//! 4. Optionally, [`weights`]: smooth skin weights for every part, to
//!    part-named bones, so joints bend instead of hinging.
//! 5. Write the parts out: [`mesh_v2`] (or the skinned [`mesh_v4`]) for a
//!    local preview through the asset cache, [`gltf`] (or the skinned
//!    [`gltf_skinned`]) for Open Cloud's model import.
//!
//! Roblox space throughout: +Y up, the character facing −Z, its left on −X,
//! studs.
//!
//! Nothing here is learnt: the method is the volumetric landmarking of the
//! auto-rig research notes, chosen because it needs no training data, runs
//! well under a second and fails visibly (an error naming what was not
//! found) rather than silently, so a person can fix the markers.

mod accessory;
mod fit;
mod mesh;
mod orient;
mod read;
mod rig;
mod skin;
mod split;
mod voxel;
mod write;

pub use fit::{fit, fit_with, mirror, Extremities, Fit, FitOptions};
pub use mesh::Mesh;
pub use orient::{Axis, Orientation};
pub use read::{read, read_bytes, Format};
pub use rig::{Joint, Part, Side};
pub use skin::{bones_of, weights, PartSkin, Skin, MAX_INFLUENCES};
pub use split::{split, split_with, Body, PartMesh, BUDGETS};
pub use write::{gltf, gltf_skinned, mesh_v2, mesh_v4};

/// Why a mesh could not be read, fitted or split.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn\u{2019}t read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("can\u{2019}t read .{0} files: use .gltf, .glb, .obj or .fbx")]
    UnsupportedFormat(String),
    #[error("{0}")]
    Format(String),
    #[error("the mesh has no triangles")]
    Empty,
    #[error("the height must be a positive number of studs")]
    Height,
    #[error("the up and front axes must be perpendicular")]
    Orientation,
    #[error("found {found} extremities, expected 5 (head, two hands, two feet): place the markers by hand")]
    Extremities { found: usize },
    #[error("{0} came out empty: move its markers so its cuts cross the mesh")]
    EmptyPart(Part),
}

#[cfg(test)]
mod tests;

/// The synthetic humanoid with known joints the tests rig, also for other
/// crates' tests (feature `test-humanoid`).
#[cfg(any(test, feature = "test-humanoid"))]
#[path = "tests/humanoid.rs"]
pub mod test_humanoid;
