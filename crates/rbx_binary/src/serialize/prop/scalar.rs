//! Encoders for scalar property types, the encode counterpart of `chunks::prop::scalar`.

use rbx_dom::{PhysicalProperties, Variant};

use crate::codec::{interleave_u32, interleave_zigzag_i32, interleave_zigzag_i64};
use crate::serialize::prop::map_dense;
use crate::serialize::sstr::{SharedStringTable, SHARED_STRING_TYPE_ID};
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

const STRING_TYPE_ID: u8 = 0x01;
const PHYSICAL_PROPERTIES_DEFAULT: u8 = 0b10;
const PHYSICAL_PROPERTIES_CUSTOM: u8 = 0b01;

// Strings are sequential and length-prefixed, so `Unknown` (non-UTF8 bytes read
// through the same STRING wire type) can be mixed in per instance without changing
// the framing: only the byte source differs.
pub(super) fn strings(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let mut writer = Writer::new();
    for value in values {
        let bytes: &[u8] = match value {
            Some(Variant::String(s)) => s.as_bytes(),
            Some(Variant::Unknown { type_id, raw }) if *type_id == STRING_TYPE_ID => raw,
            Some(_) => return Err(SerializeError::mismatch(class, name)),
            None => return Err(SerializeError::missing(class, name)),
        };
        writer.sized_bytes(bytes);
    }
    Ok(writer.into_bytes())
}

pub(super) fn bools(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let bools = map_dense(class, name, values, |v| match v {
        Variant::Bool(b) => Some(*b),
        _ => None,
    })?;
    Ok(bools.into_iter().map(u8::from).collect())
}

pub(super) fn int32s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let ints = map_dense(class, name, values, |v| match v {
        Variant::Int32(i) => Some(*i),
        _ => None,
    })?;
    Ok(interleave_zigzag_i32(&ints))
}

pub(super) fn int64s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let ints = map_dense(class, name, values, |v| match v {
        Variant::Int64(i) => Some(*i),
        _ => None,
    })?;
    Ok(interleave_zigzag_i64(&ints))
}

pub(super) fn security_capabilities(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let bits = map_dense(class, name, values, |v| match v {
        Variant::SecurityCapabilities(b) => Some(*b as i64),
        _ => None,
    })?;
    Ok(interleave_zigzag_i64(&bits))
}

pub(super) fn brick_colors(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let colors = map_dense(class, name, values, |v| match v {
        Variant::BrickColor(c) => Some(*c),
        _ => None,
    })?;
    Ok(interleave_u32(&colors))
}

pub(super) fn float32s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let floats = map_dense(class, name, values, |v| match v {
        Variant::Float32(f) => Some(*f),
        _ => None,
    })?;
    Ok(crate::codec::interleave_f32(&floats))
}

pub(super) fn float64s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let floats = map_dense(class, name, values, |v| match v {
        Variant::Float64(f) => Some(*f),
        _ => None,
    })?;
    let mut writer = Writer::new();
    for value in floats {
        writer.f64(value);
    }
    Ok(writer.into_bytes())
}

pub(super) fn enums(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let enums = map_dense(class, name, values, |v| match v {
        Variant::Enum(e) => Some(*e),
        _ => None,
    })?;
    Ok(interleave_u32(&enums))
}

// `Ref` is the one non-`Unknown` type allowed to be absent: a missing key means the
// original file had a null referent, which round-trips as -1 with no error.
pub(super) fn refs(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let raw: Vec<i32> = values
        .iter()
        .map(|value| match value {
            Some(Variant::Ref(r)) => Ok(r.value() as i32),
            None => Ok(-1),
            Some(_) => Err(SerializeError::mismatch(class, name)),
        })
        .collect::<Result<_, _>>()?;
    Ok(crate::codec::encode_referents(&raw))
}

pub(super) fn color3_uint8s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let colors = map_dense(class, name, values, |v| match v {
        Variant::Color3uint8 { r, g, b } => Some((*r, *g, *b)),
        _ => None,
    })?;
    let mut writer = Writer::new();
    for &(r, _, _) in &colors {
        writer.u8(r);
    }
    for &(_, g, _) in &colors {
        writer.u8(g);
    }
    for &(_, _, b) in &colors {
        writer.u8(b);
    }
    Ok(writer.into_bytes())
}

// The custom path never writes the optional sixth (AcousticAbsorption) float: the DOM
// has no field for it, and the flag byte simply omits the bit that would ask a reader
// to expect one.
pub(super) fn physical_properties(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let props = map_dense(class, name, values, |v| match v {
        Variant::PhysicalProperties(p) => Some(*p),
        _ => None,
    })?;
    let mut writer = Writer::new();
    for prop in props {
        match prop {
            PhysicalProperties::Default => writer.u8(PHYSICAL_PROPERTIES_DEFAULT),
            PhysicalProperties::Custom {
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            } => {
                writer.u8(PHYSICAL_PROPERTIES_CUSTOM);
                writer.f32(density);
                writer.f32(friction);
                writer.f32(elasticity);
                writer.f32(friction_weight);
                writer.f32(elasticity_weight);
            }
        }
    }
    Ok(writer.into_bytes())
}

// Three shapes can land in the same group: `Variant::SharedString(i)` is a raw index
// that never resolved against a table on read (no content to intern, so it is written
// back verbatim), while `Variant::String`/`Variant::Unknown{0x1C,..}` are resolved
// content that must round-trip through the very same table this file's SSTR chunk was
// built from (`shared`), so their index has to match what that table assigned.
pub(super) fn shared_strings(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
    shared: &SharedStringTable,
) -> Result<Vec<u8>, SerializeError> {
    let indices = map_dense(class, name, values, |v| match v {
        Variant::SharedString(i) => Some(*i),
        Variant::String(text) => shared.index_of(text.as_bytes()),
        Variant::Unknown { type_id, raw } if *type_id == SHARED_STRING_TYPE_ID => {
            shared.index_of(raw)
        }
        _ => None,
    })?;
    Ok(interleave_u32(&indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::Ref;

    fn decoded(type_id: u8, count: usize, payload: &[u8]) -> Vec<Option<rbx_dom::Variant>> {
        let header = PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        };
        decode(&header, count, &[])
    }

    #[test]
    fn strings_mix_utf8_and_unknown_and_round_trip() {
        let values = vec![
            Some(Variant::String("hi".to_owned())),
            Some(Variant::Unknown {
                type_id: STRING_TYPE_ID,
                raw: vec![0xFF, 0xFE],
            }),
        ];
        let payload = strings("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(STRING_TYPE_ID, 2, &payload), values);
    }

    #[test]
    fn bools_round_trip() {
        let values = vec![Some(Variant::Bool(true)), Some(Variant::Bool(false))];
        let payload = bools("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x02, 2, &payload), values);
    }

    #[test]
    fn int32_round_trips_negative_values() {
        let values = vec![Some(Variant::Int32(-5)), Some(Variant::Int32(1_000_000))];
        let payload = int32s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x03, 2, &payload), values);
    }

    #[test]
    fn refs_write_minus_one_for_a_missing_key() {
        let values = vec![Some(Variant::Ref(Ref::new(9))), None];
        let payload = refs("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x13, 2, &payload), values);
    }

    #[test]
    fn color3_uint8_round_trips_three_planes() {
        let values = vec![
            Some(Variant::Color3uint8 { r: 1, g: 2, b: 3 }),
            Some(Variant::Color3uint8 { r: 4, g: 5, b: 6 }),
        ];
        let payload = color3_uint8s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x1A, 2, &payload), values);
    }

    #[test]
    fn physical_properties_default_and_custom_round_trip() {
        let values = vec![
            Some(Variant::PhysicalProperties(PhysicalProperties::Default)),
            Some(Variant::PhysicalProperties(PhysicalProperties::Custom {
                density: 0.7,
                friction: 0.3,
                elasticity: 0.5,
                friction_weight: 1.0,
                elasticity_weight: 2.0,
            })),
        ];
        let payload = physical_properties("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x19, 2, &payload), values);
    }

    #[test]
    fn shared_string_round_trips_via_an_empty_table() {
        let values = vec![Some(Variant::SharedString(4))];
        let payload = shared_strings("Test", "Prop", &values, &SharedStringTable::new()).unwrap();
        assert_eq!(decoded(0x1C, 1, &payload), values);
    }

    #[test]
    fn shared_string_resolves_through_a_built_table() {
        let mut shared = SharedStringTable::new();
        // Mirrors the table-building pass: every resolvable payload in the group
        // (empty string included) is interned before any index is assigned.
        let empty_index = shared.intern(b"");
        let mesh_index = shared.intern(b"mesh-bytes");

        let values = vec![
            Some(Variant::String(String::new())),
            Some(Variant::Unknown {
                type_id: SHARED_STRING_TYPE_ID,
                raw: b"mesh-bytes".to_vec(),
            }),
        ];
        let payload = shared_strings("Test", "Prop", &values, &shared).unwrap();

        assert_eq!(
            payload,
            interleave_u32(&[empty_index, mesh_index]),
            "each entry must resolve to the index the table assigned its bytes"
        );
    }
}
