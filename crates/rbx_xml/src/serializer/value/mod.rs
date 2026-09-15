//! Dispatches one property's `Variant` to its encoder, by kind. The inverse of
//! `crate::value`'s dispatch-by-tag-name, one encoder per `Variant` shape rather
//! than per XML tag name (a `Variant` never remembers which of two decoder tags
//! produced it, e.g. `string`/`ProtectedString`, so there is only one way back).

mod cframe;
mod content;
mod flags;
mod font;
mod identity;
mod refs;
mod scalar;
mod sequence;
mod udim;
mod vector;

use rbx_dom::Variant;

use crate::error::XmlError;
use crate::serializer::writer::Writer;
use crate::value::STRING_TYPE_ID;

/// Encodes one property as a child of the currently open `<Properties>` element.
///
/// `Unknown` blobs whose `type_id` is `STRING_TYPE_ID` are written as
/// `BinaryString` (the only case `scalar::binary_string` round-trips losslessly,
/// since XML's `BinaryString` carries no side channel for a wire type id — see
/// its doc comment). Any other `type_id` returns `Unsupported("Unknown")` rather
/// than silently writing a blob that would read back tagged `STRING_TYPE_ID`.
///
/// Returns `Unsupported("SharedString")` for `Variant::SharedString(u32)`: this is
/// only ever an *unresolved* SSTR table index (a resolved one already decodes to
/// `Variant::String`, see `rbx_binary`'s own `shared_strings`), so there are no
/// bytes here to write into a `SharedString` table entry.
/// Writes the DOM's dedicated `name` field back as an ordinary `string` property,
/// the inverse of `deserializer`'s `NAME_PROPERTY` redirect.
pub(crate) fn string_property(writer: &mut Writer, name_value: &str) {
    scalar::string(writer, "Name", name_value);
}

pub(crate) fn encode(writer: &mut Writer, name: &str, value: &Variant) -> Result<(), XmlError> {
    match value {
        Variant::String(text) => scalar::string(writer, name, text),
        Variant::Bool(b) => scalar::bool_value(writer, name, *b),
        Variant::Int32(v) => scalar::int32(writer, name, *v),
        Variant::Int64(v) => scalar::int64(writer, name, *v),
        Variant::Float32(v) => scalar::float32(writer, name, *v),
        Variant::Float64(v) => scalar::float64(writer, name, *v),
        Variant::BrickColor(v) => scalar::brick_color(writer, name, *v),
        Variant::Enum(v) => scalar::enum_value(writer, name, *v),
        Variant::Color3(c) => vector::color3(writer, name, c),
        Variant::Color3uint8 { r, g, b } => vector::color3_uint8(writer, name, *r, *g, *b),
        Variant::Vector2(v) => vector::vector2(writer, name, v),
        Variant::Vector3(v) => vector::vector3(writer, name, v),
        Variant::Vector3int16 { x, y, z } => vector::vector3int16(writer, name, *x, *y, *z),
        Variant::Ray { origin, direction } => vector::ray(writer, name, origin, direction),
        Variant::Rect(rect) => vector::rect2d(writer, name, rect),
        Variant::Faces(f) => flags::faces(writer, name, f),
        Variant::Axes(a) => flags::axes(writer, name, a),
        Variant::PhysicalProperties(p) => flags::physical_properties(writer, name, p),
        Variant::SecurityCapabilities(v) => flags::security_capabilities(writer, name, *v),
        Variant::CFrame(c) => cframe::cframe(writer, name, c),
        Variant::OptionalCFrame(c) => cframe::optional_cframe(writer, name, c),
        Variant::UDim(u) => udim::udim(writer, name, u),
        Variant::UDim2(u) => udim::udim2(writer, name, u),
        Variant::NumberRange(v) => sequence::number_range(writer, name, v),
        Variant::NumberSequence(v) => sequence::number_sequence(writer, name, v),
        Variant::ColorSequence(v) => sequence::color_sequence(writer, name, v),
        Variant::UniqueId(v) => identity::unique_id(writer, name, v),
        Variant::Font(f) => font::font(writer, name, f),
        Variant::Content(c) => content::content(writer, name, c),
        Variant::Ref(r) => refs::reference(writer, name, *r),
        Variant::SharedString(_) => return Err(XmlError::Unsupported("SharedString")),
        Variant::Unknown { type_id, raw } if *type_id == STRING_TYPE_ID => {
            scalar::binary_string(writer, name, raw)
        }
        Variant::Unknown { .. } => return Err(XmlError::Unsupported("Unknown")),
    }
    Ok(())
}
