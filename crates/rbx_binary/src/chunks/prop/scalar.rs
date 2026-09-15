//! Decoders for scalar property types (primitives, colors, refs, enums, etc.).

use rbx_dom::{PhysicalProperties, Ref, Variant};

use super::PropValues;
use crate::codec::{zigzag_i32, zigzag_i64, Reader};
use crate::error::BinaryError;

const STRING_TYPE_ID: u8 = 0x01;
const SHARED_STRING_TYPE_ID: u8 = 0x1C;
// Bit 0 of the PhysicalProperties flag byte: the instance overrides the material
// defaults. Bit 1 adds a sixth float (AcousticAbsorption).
const PHYSICAL_PROPERTIES_CUSTOM: u8 = 0b01;
const PHYSICAL_PROPERTIES_ACOUSTIC: u8 = 0b10;

fn wrap(values: Vec<Variant>) -> PropValues {
    values.into_iter().map(Some).collect()
}

// Strings are the only sequential (non-interleaved) variable-length payload:
// i32 length + bytes, repeated once per instance.
pub(super) fn strings(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| Ok(Some(string_value(reader.sized_bytes()?, STRING_TYPE_ID))))
        .collect()
}

// Property values of type String also carry binary blobs (AttributesSerialize,
// mesh data), so non-UTF-8 content is kept verbatim instead of being mangled by
// a lossy conversion.
fn string_value(bytes: &[u8], type_id: u8) -> Variant {
    match std::str::from_utf8(bytes) {
        Ok(text) => Variant::String(text.to_owned()),
        Err(_) => Variant::Unknown {
            type_id,
            raw: bytes.to_vec(),
        },
    }
}

pub(super) fn bools(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .take(count)?
            .iter()
            .map(|&byte| Variant::Bool(byte != 0))
            .collect(),
    ))
}

pub(super) fn int32s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u32(count)?
            .into_iter()
            .map(|raw| Variant::Int32(zigzag_i32(raw)))
            .collect(),
    ))
}

pub(super) fn int64s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u64(count)?
            .into_iter()
            .map(|raw| Variant::Int64(zigzag_i64(raw)))
            .collect(),
    ))
}

// Wire-identical to Int64 (interleaved, zigzag): Roblox writes the capability
// bitfield through its signed-integer path even though every bit is a flag.
// TODO: both fixtures only carry 0, so the zigzag step is unverifiable from them;
// it follows rbx-dom, whose writer round-trips the same transformation.
pub(super) fn security_capabilities(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u64(count)?
            .into_iter()
            .map(|raw| Variant::SecurityCapabilities(zigzag_i64(raw) as u64))
            .collect(),
    ))
}

// A palette index, not a color: no zigzag and no bit rotation, unlike every other
// interleaved number in the format.
pub(super) fn brick_colors(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u32(count)?
            .into_iter()
            .map(Variant::BrickColor)
            .collect(),
    ))
}

pub(super) fn float32s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_f32(count)?
            .into_iter()
            .map(Variant::Float32)
            .collect(),
    ))
}

// Float64 breaks the pattern of every other numeric type: Roblox writes plain
// little-endian doubles with no interleaving and no bit rotation.
pub(super) fn float64s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| Ok(Some(Variant::Float64(reader.f64()?))))
        .collect()
}

pub(super) fn enums(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u32(count)?
            .into_iter()
            .map(Variant::Enum)
            .collect(),
    ))
}

pub(super) fn refs(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(reader
        .referents(count)?
        .into_iter()
        .map(|referent| {
            // -1 is Roblox's null referent; absence of the property is how the
            // DOM represents it, since Ref is a plain u32 newtype.
            u32::try_from(referent)
                .ok()
                .map(|id| Variant::Ref(Ref::new(id)))
        })
        .collect())
}

// Three byte planes (every red, then every green, then every blue), like the
// component blocks of Color3 but without any per-value transformation.
pub(super) fn color3_uint8s(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    let reds = reader.take(count)?.to_vec();
    let greens = reader.take(count)?.to_vec();
    let blues = reader.take(count)?;

    Ok(wrap(
        (0..count)
            .map(|i| Variant::Color3uint8 {
                r: reds[i],
                g: greens[i],
                b: blues[i],
            })
            .collect(),
    ))
}

// Per instance: one flag byte immediately followed by that instance's floats,
// so the flags are *not* grouped in a leading block. The flag is a bitfield, not
// a boolean: 0x02 (acoustic bit alone, what both test files write) still means
// "no custom properties" and carries no floats.
// TODO: the custom paths (0x01 with five floats, 0x03 with six) are unexercised
// by our test files; they need a fixture with real CustomPhysicalProperties.
pub(super) fn physical_properties(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            let flags = reader.u8()?;
            if flags & PHYSICAL_PROPERTIES_CUSTOM == 0 {
                return Ok(Some(Variant::PhysicalProperties(
                    PhysicalProperties::Default,
                )));
            }

            let custom = PhysicalProperties::Custom {
                density: reader.f32()?,
                friction: reader.f32()?,
                elasticity: reader.f32()?,
                friction_weight: reader.f32()?,
                elasticity_weight: reader.f32()?,
            };
            // AcousticAbsorption has no field in the DOM yet; it is consumed to
            // keep the reader aligned on the next instance.
            if flags & PHYSICAL_PROPERTIES_ACOUSTIC != 0 {
                reader.f32()?;
            }
            Ok(Some(Variant::PhysicalProperties(custom)))
        })
        .collect()
}

// Plain interleaved u32 indices into the file's SSTR table (no zigzag, no
// delta). Resolving them here keeps the table private to this crate; an index we
// cannot resolve degrades to the raw index so nothing is silently dropped.
pub(super) fn shared_strings(
    reader: &mut Reader<'_>,
    count: usize,
    shared: &[Vec<u8>],
) -> Result<PropValues, BinaryError> {
    Ok(wrap(
        reader
            .interleaved_u32(count)?
            .into_iter()
            .map(|index| match shared.get(index as usize) {
                Some(bytes) => string_value(bytes, SHARED_STRING_TYPE_ID),
                None => Variant::SharedString(index),
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_read_sequential_length_prefixed_values() {
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(b"hi");
        data.extend_from_slice(&0i32.to_le_bytes());

        let values = strings(&mut Reader::new(&data), 2).unwrap();

        assert_eq!(values[0], Some(Variant::String("hi".to_owned())));
        assert_eq!(values[1], Some(Variant::String(String::new())));
    }

    #[test]
    fn non_utf8_string_keeps_raw_bytes() {
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&[0xFF, 0xFE]);

        let values = strings(&mut Reader::new(&data), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Unknown {
                type_id: STRING_TYPE_ID,
                raw: vec![0xFF, 0xFE]
            })
        );
    }

    #[test]
    fn null_referent_yields_no_value() {
        // Single element, zigzag(1) = -1.
        let values = refs(&mut Reader::new(&[0, 0, 0, 1]), 1).unwrap();
        assert_eq!(values, vec![None]);
    }

    #[test]
    fn shared_string_resolves_against_the_table() {
        let table = vec![b"tagged".to_vec()];
        let values = shared_strings(&mut Reader::new(&[0, 0, 0, 0]), 1, &table).unwrap();

        assert_eq!(values[0], Some(Variant::String("tagged".to_owned())));
    }

    #[test]
    fn shared_string_out_of_range_keeps_the_index() {
        let values = shared_strings(&mut Reader::new(&[0, 0, 0, 4]), 1, &[]).unwrap();
        assert_eq!(values[0], Some(Variant::SharedString(4)));
    }

    // SpawnLocation.TeamColor from TestPlace.rbxl: 194, "Medium stone grey".
    #[test]
    fn brick_color_is_an_untransformed_big_endian_index() {
        let values = brick_colors(&mut Reader::new(&[0x00, 0x00, 0x00, 0xC2]), 1).unwrap();
        assert_eq!(values[0], Some(Variant::BrickColor(194)));
    }

    #[test]
    fn security_capabilities_undo_the_zigzag() {
        let payload = [0u8, 0, 0, 0, 0, 0, 0, 4];
        let values = security_capabilities(&mut Reader::new(&payload), 1).unwrap();
        assert_eq!(values[0], Some(Variant::SecurityCapabilities(2)));
    }

    #[test]
    fn color3_uint8_reads_three_byte_planes() {
        let payload = [10u8, 20, 30, 40, 50, 60];
        let values = color3_uint8s(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values,
            vec![
                Some(Variant::Color3uint8 {
                    r: 10,
                    g: 30,
                    b: 50
                }),
                Some(Variant::Color3uint8 {
                    r: 20,
                    g: 40,
                    b: 60
                }),
            ]
        );
    }

    #[test]
    fn physical_properties_custom_reads_five_floats_inline() {
        let mut payload = vec![0b01];
        for value in [0.7f32, 0.3, 0.5, 1.0, 2.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0b10);

        let values = physical_properties(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::PhysicalProperties(PhysicalProperties::Custom {
                density: 0.7,
                friction: 0.3,
                elasticity: 0.5,
                friction_weight: 1.0,
                elasticity_weight: 2.0,
            }))
        );
        assert_eq!(
            values[1],
            Some(Variant::PhysicalProperties(PhysicalProperties::Default))
        );
    }

    #[test]
    fn physical_properties_without_floats_are_default() {
        let values = physical_properties(&mut Reader::new(&[2, 2]), 2).unwrap();

        assert_eq!(
            values,
            vec![
                Some(Variant::PhysicalProperties(PhysicalProperties::Default)),
                Some(Variant::PhysicalProperties(PhysicalProperties::Default)),
            ]
        );
    }
}
