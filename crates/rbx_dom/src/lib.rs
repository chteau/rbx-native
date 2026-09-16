//! In-memory DOM representation of a Roblox instance tree.
//!
//! Provides a weak reference-based tree structure that maps directly onto
//! the binary file format without circular strong references.

mod change;
mod dom;
mod error;
mod instance;
mod reference;
mod variant;

pub use change::Change;
pub use dom::{Snapshot, WeakDom};
pub use error::DomError;
pub use instance::Instance;
pub use reference::Ref;
pub use variant::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    FontStyle, NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, UDim,
    UDim2, UniqueId, Variant, Vector2Data, Vector3Data,
};
