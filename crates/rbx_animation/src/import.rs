//! Bringing animations made elsewhere (Blender, mostly) into a clip.

pub mod fbx;
pub mod gltf;
pub mod rbxanims;
pub mod skeleton;

use crate::clip::Clip;
use crate::frame::Frame;
use crate::rig::Rig;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Format(String),
    #[error("the file animates no bone named after one of the rig's parts")]
    NothingMatched,
}

/// A file format the importer reads, picked by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Fbx,
    Gltf,
}

impl Format {
    pub fn of(path: &str) -> Option<Format> {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".fbx") {
            Some(Format::Fbx)
        } else if lower.ends_with(".glb") || lower.ends_with(".gltf") {
            Some(Format::Gltf)
        } else {
            None
        }
    }
}

/// Positions within this many studs, and turns within this many radians, are
/// the same when trimming a baked file's keys.
const OPTIMIZE_TOLERANCE: f32 = 1e-3;

/// Reads `bytes` as `format` and retargets the result onto `rig`, whose root
/// stands at `root`. A baked file's redundant keys are trimmed.
pub fn retargeted(
    format: Format,
    bytes: &[u8],
    external: &dyn Fn(&str) -> Option<Vec<u8>>,
    rig: &Rig,
    root: Frame,
    name: &str,
) -> Result<Clip, ImportError> {
    let skeleton = match format {
        Format::Fbx => fbx::read(bytes)?,
        Format::Gltf => gltf::read(bytes, 0, external)?,
    };
    let mut clip = skeleton::retarget(&skeleton, rig, root, name);
    if clip.keyframes.is_empty() {
        return Err(ImportError::NothingMatched);
    }
    skeleton::optimize(&mut clip, OPTIMIZE_TOLERANCE);
    Ok(clip)
}
