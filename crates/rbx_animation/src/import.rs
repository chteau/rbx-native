//! Bringing animations made elsewhere (Blender, mostly) into a clip.

pub mod rbxanims;
pub mod skeleton;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Format(String),
    #[error("the file has no animation for any of the rig's joints")]
    NothingMatched,
}
