//! Property value types used in the Roblox instance tree.

mod asset;
mod flags;
mod geometry;
mod sequence;

pub use asset::{Content, Font, FontStyle, UniqueId};
pub use flags::{Axes, Faces};
pub use geometry::{
    CFrameData, Color3Data, PhysicalProperties, Rect, UDim, UDim2, Vector2Data, Vector3Data,
};
pub use sequence::{
    ColorSequence, ColorSequenceKeypoint, NumberRange, NumberSequence, NumberSequenceKeypoint,
};

use crate::reference::Ref;

/// A property value in the Roblox object hierarchy.
///
/// Each variant corresponds to a property type ID from the binary format. The `Unknown`
/// variant is the fallback for type IDs the parser does not yet decode, allowing graceful
/// degradation instead of panicking on new or exotic property types.
#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    String(String),
    Bool(bool),
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    // Palette index into Roblox's fixed BrickColor table, not a color.
    BrickColor(u32),
    Color3(Color3Data),
    Color3uint8 {
        r: u8,
        g: u8,
        b: u8,
    },
    Vector2(Vector2Data),
    Vector3(Vector3Data),
    Vector3int16 {
        x: i16,
        y: i16,
        z: i16,
    },
    Ray {
        origin: Vector3Data,
        direction: Vector3Data,
    },
    Faces(Faces),
    Axes(Axes),
    CFrame(CFrameData),
    // `None` is a CFrame the file explicitly marks as absent, which is distinct
    // from the property being missing altogether.
    OptionalCFrame(Option<CFrameData>),
    Enum(u32),
    Ref(Ref),
    NumberSequence(NumberSequence),
    ColorSequence(ColorSequence),
    NumberRange(NumberRange),
    Rect(Rect),
    PhysicalProperties(PhysicalProperties),
    // Raw index into the file's SSTR table; resolved against that table later.
    SharedString(u32),
    UDim(UDim),
    UDim2(UDim2),
    UniqueId(UniqueId),
    Font(Font),
    // Bitfield of script capabilities; kept unsigned because the bits are flags,
    // even though the wire format encodes them as a signed integer.
    SecurityCapabilities(u64),
    Content(Content),
    Unknown {
        type_id: u8,
        raw: Vec<u8>,
    },
}
