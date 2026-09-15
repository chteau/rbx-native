//! PROP payload encoding, the encode counterpart of `chunks::prop`.
//!
//! Each submodule mirrors its decoder one-for-one; see `chunks::prop::type_id` for the
//! wire ids this module writes (kept as a private copy here since that one is private
//! to the decoder's own module).

mod cframe;
mod content;
mod font;
mod identity;
mod misc;
mod scalar;
mod sequence;
mod udim;
mod vector;

use rbx_dom::Variant;

use super::sstr::SharedStringTable;
use super::SerializeError;

// Wire type ids, exactly as `chunks::prop::type_id` defines them.
mod type_id {
    pub(super) const STRING: u8 = 0x01;
    pub(super) const BOOL: u8 = 0x02;
    pub(super) const INT32: u8 = 0x03;
    pub(super) const FLOAT32: u8 = 0x04;
    pub(super) const FLOAT64: u8 = 0x05;
    pub(super) const UDIM: u8 = 0x06;
    pub(super) const UDIM2: u8 = 0x07;
    pub(super) const RAY: u8 = 0x08;
    pub(super) const FACES: u8 = 0x09;
    pub(super) const AXES: u8 = 0x0A;
    pub(super) const BRICK_COLOR: u8 = 0x0B;
    pub(super) const COLOR3: u8 = 0x0C;
    pub(super) const VECTOR2: u8 = 0x0D;
    pub(super) const VECTOR3: u8 = 0x0E;
    pub(super) const CFRAME: u8 = 0x10;
    pub(super) const ENUM: u8 = 0x12;
    pub(super) const REF: u8 = 0x13;
    pub(super) const VECTOR3INT16: u8 = 0x14;
    pub(super) const NUMBER_SEQUENCE: u8 = 0x15;
    pub(super) const COLOR_SEQUENCE: u8 = 0x16;
    pub(super) const NUMBER_RANGE: u8 = 0x17;
    pub(super) const RECT: u8 = 0x18;
    pub(super) const PHYSICAL_PROPERTIES: u8 = 0x19;
    pub(super) const COLOR3_UINT8: u8 = 0x1A;
    pub(super) const INT64: u8 = 0x1B;
    pub(super) const SHARED_STRING: u8 = 0x1C;
    pub(super) const OPTIONAL_CFRAME: u8 = 0x1E;
    pub(super) const UNIQUE_ID: u8 = 0x1F;
    pub(super) const FONT: u8 = 0x20;
    pub(super) const SECURITY_CAPABILITIES: u8 = 0x21;
    pub(super) const CONTENT: u8 = 0x22;
}

/// Encodes every instance's value for one (class, property) pair.
///
/// `values` is positional against the class's INST referent list, `None` marking an
/// instance with no value for this property (only representable for `Ref`-typed
/// properties, via the null referent).
pub(crate) fn encode(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
    shared: &SharedStringTable,
) -> Result<(u8, Vec<u8>), SerializeError> {
    // `Variant::Unknown` can stand in for a type this crate never decodes losslessly
    // (a non-UTF8 String) or a genuinely unknown wire type id; either way, *that* id
    // decides the framing, so it takes priority over any other Some entry's shape.
    if let Some(id) = unknown_type_id(class, name, values)? {
        return if id == type_id::STRING {
            Ok((type_id::STRING, scalar::strings(class, name, values)?))
        } else if id == type_id::SHARED_STRING {
            // Unlike other `Unknown` blobs, a SharedString group is never a uniform
            // shape: sibling instances legitimately decode to `Variant::String` (a
            // resolved entry that happened to be valid UTF-8, empty included) as well
            // as `Variant::Unknown` (binary content), so it gets its own encoder
            // instead of `misc::unknown_blob`'s single-shape, fixed-stride one.
            Ok((
                type_id::SHARED_STRING,
                scalar::shared_strings(class, name, values, shared)?,
            ))
        } else {
            Ok((id, misc::unknown_blob(class, name, id, values)?))
        };
    }

    let kind = values
        .iter()
        .find_map(|value| value.as_ref())
        .ok_or_else(|| SerializeError::missing(class, name))?;

    match kind {
        Variant::String(_) => Ok((type_id::STRING, scalar::strings(class, name, values)?)),
        Variant::Bool(_) => Ok((type_id::BOOL, scalar::bools(class, name, values)?)),
        Variant::Int32(_) => Ok((type_id::INT32, scalar::int32s(class, name, values)?)),
        Variant::Int64(_) => Ok((type_id::INT64, scalar::int64s(class, name, values)?)),
        Variant::Float32(_) => Ok((type_id::FLOAT32, scalar::float32s(class, name, values)?)),
        Variant::Float64(_) => Ok((type_id::FLOAT64, scalar::float64s(class, name, values)?)),
        Variant::BrickColor(_) => Ok((
            type_id::BRICK_COLOR,
            scalar::brick_colors(class, name, values)?,
        )),
        Variant::Color3(_) => Ok((type_id::COLOR3, vector::color3s(class, name, values)?)),
        Variant::Color3uint8 { .. } => Ok((
            type_id::COLOR3_UINT8,
            scalar::color3_uint8s(class, name, values)?,
        )),
        Variant::Vector2(_) => Ok((type_id::VECTOR2, vector::vector2s(class, name, values)?)),
        Variant::Vector3(_) => Ok((type_id::VECTOR3, vector::vector3s(class, name, values)?)),
        Variant::Vector3int16 { .. } => Ok((
            type_id::VECTOR3INT16,
            misc::vector3int16s(class, name, values)?,
        )),
        Variant::Ray { .. } => Ok((type_id::RAY, misc::rays(class, name, values)?)),
        Variant::Faces(_) => Ok((type_id::FACES, misc::faces(class, name, values)?)),
        Variant::Axes(_) => Ok((type_id::AXES, misc::axes(class, name, values)?)),
        Variant::CFrame(_) => Ok((type_id::CFRAME, cframe::cframes(class, name, values)?)),
        Variant::OptionalCFrame(_) => Ok((
            type_id::OPTIONAL_CFRAME,
            cframe::optional_cframes(class, name, values)?,
        )),
        Variant::Enum(_) => Ok((type_id::ENUM, scalar::enums(class, name, values)?)),
        Variant::Ref(_) => Ok((type_id::REF, scalar::refs(class, name, values)?)),
        Variant::NumberSequence(_) => Ok((
            type_id::NUMBER_SEQUENCE,
            sequence::number_sequences(class, name, values)?,
        )),
        Variant::ColorSequence(_) => Ok((
            type_id::COLOR_SEQUENCE,
            sequence::color_sequences(class, name, values)?,
        )),
        Variant::NumberRange(_) => Ok((
            type_id::NUMBER_RANGE,
            sequence::number_ranges(class, name, values)?,
        )),
        Variant::Rect(_) => Ok((type_id::RECT, vector::rects(class, name, values)?)),
        Variant::PhysicalProperties(_) => Ok((
            type_id::PHYSICAL_PROPERTIES,
            scalar::physical_properties(class, name, values)?,
        )),
        Variant::SharedString(_) => Ok((
            type_id::SHARED_STRING,
            scalar::shared_strings(class, name, values, shared)?,
        )),
        Variant::UDim(_) => Ok((type_id::UDIM, udim::udims(class, name, values)?)),
        Variant::UDim2(_) => Ok((type_id::UDIM2, udim::udim2s(class, name, values)?)),
        Variant::UniqueId(_) => Ok((
            type_id::UNIQUE_ID,
            identity::unique_ids(class, name, values)?,
        )),
        Variant::Font(_) => Ok((type_id::FONT, font::fonts(class, name, values)?)),
        Variant::SecurityCapabilities(_) => Ok((
            type_id::SECURITY_CAPABILITIES,
            scalar::security_capabilities(class, name, values)?,
        )),
        Variant::Content(_) => Ok((type_id::CONTENT, content::contents(class, name, values)?)),
        // Unreachable: every `Unknown` is handled by `unknown_type_id` above.
        Variant::Unknown { .. } => Err(SerializeError::Unsupported("Unknown")),
    }
}

// Finds the wire type id an `Unknown` entry carries, requiring every `Unknown` entry
// in the group to agree on it (a class+property pair has exactly one wire type).
fn unknown_type_id(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Option<u8>, SerializeError> {
    let mut found = None;
    for value in values {
        if let Some(Variant::Unknown { type_id, .. }) = value {
            match found {
                None => found = Some(*type_id),
                Some(existing) if existing == *type_id => {}
                Some(_) => return Err(SerializeError::mismatch(class, name)),
            }
        }
    }
    Ok(found)
}

/// Extracts one field from every value, requiring every instance to carry this
/// property: the binary format has no "missing" representation for non-`Ref` types.
pub(super) fn map_dense<T>(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
    mut extract: impl FnMut(&Variant) -> Option<T>,
) -> Result<Vec<T>, SerializeError> {
    values
        .iter()
        .map(|value| match value {
            Some(variant) => extract(variant).ok_or_else(|| SerializeError::mismatch(class, name)),
            None => Err(SerializeError::missing(class, name)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_bool_to_the_right_wire_type() {
        let values = vec![Some(Variant::Bool(true)), Some(Variant::Bool(false))];
        let (id, payload) = encode("Part", "Anchored", &values, &SharedStringTable::new()).unwrap();

        assert_eq!(id, type_id::BOOL);
        assert_eq!(payload, vec![1, 0]);
    }

    #[test]
    fn missing_non_ref_value_is_rejected() {
        let values = vec![Some(Variant::Bool(true)), None];
        let err = encode("Part", "Anchored", &values, &SharedStringTable::new()).unwrap_err();
        assert!(matches!(err, SerializeError::InconsistentProperty { .. }));
    }
}
