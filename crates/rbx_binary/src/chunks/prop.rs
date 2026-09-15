//! PROP (property) chunk parsing and type-based decoding.
//!
//! Each property type (string, bool, CFrame, etc.) has its own decoder that reads
//! arrays of values and returns them as `Variant` instances, one per instance.

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

use crate::codec::Reader;
use crate::error::BinaryError;

// `None` means "this instance has no value for the property": the only producer
// today is a null referent (-1), which `Variant::Ref` cannot express.
pub(crate) type PropValues = Vec<Option<Variant>>;

/// Metadata from the header of a PROP chunk before the typed payload.
pub(crate) struct PropHeader<'a> {
    pub(crate) class_id: i32,
    pub(crate) name: String,
    pub(crate) type_id: u8,
    pub(crate) payload: &'a [u8],
}

// Property type ids as written in the chunk; the gaps are types Roblox never
// serializes (Vector2int16, Quaternion) or that we deliberately leave to the
// `Unknown` fallback.
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

/// Parses the fixed header of a PROP chunk and returns the typed payload.
pub(crate) fn parse_header(data: &[u8]) -> Result<PropHeader<'_>, BinaryError> {
    let mut reader = Reader::new(data);

    let class_id = reader.i32()?;
    let name = reader.sized_name()?;
    let type_id = reader.u8()?;
    let consumed = data.len() - reader.remaining();

    Ok(PropHeader {
        class_id,
        name,
        type_id,
        payload: &data[consumed..],
    })
}

/// Decodes a property payload based on its type ID.
///
/// Infallible by design: an unknown or malformed type ID degrades to `Variant::Unknown`
/// so a single broken property never causes the entire file to fail.
pub(crate) fn decode(header: &PropHeader<'_>, count: usize, shared: &[Vec<u8>]) -> PropValues {
    decode_typed(header, count, shared)
        .unwrap_or_else(|_| unsupported(header.type_id, count, header.payload))
}

fn decode_typed(
    header: &PropHeader<'_>,
    count: usize,
    shared: &[Vec<u8>],
) -> Result<PropValues, BinaryError> {
    let mut reader = Reader::new(header.payload);

    match header.type_id {
        type_id::STRING => scalar::strings(&mut reader, count),
        type_id::BOOL => scalar::bools(&mut reader, count),
        type_id::INT32 => scalar::int32s(&mut reader, count),
        type_id::FLOAT32 => scalar::float32s(&mut reader, count),
        type_id::FLOAT64 => scalar::float64s(&mut reader, count),
        type_id::UDIM => udim::udims(&mut reader, count),
        type_id::UDIM2 => udim::udim2s(&mut reader, count),
        type_id::RAY => misc::rays(&mut reader, count),
        type_id::FACES => misc::faces(&mut reader, count),
        type_id::AXES => misc::axes(&mut reader, count),
        type_id::BRICK_COLOR => scalar::brick_colors(&mut reader, count),
        type_id::COLOR3 => vector::color3s(&mut reader, count),
        type_id::VECTOR2 => vector::vector2s(&mut reader, count),
        type_id::VECTOR3 => vector::vector3s(&mut reader, count),
        type_id::CFRAME => cframe::cframes(&mut reader, count),
        type_id::ENUM => scalar::enums(&mut reader, count),
        type_id::REF => scalar::refs(&mut reader, count),
        type_id::VECTOR3INT16 => misc::vector3int16s(&mut reader, count),
        type_id::NUMBER_SEQUENCE => sequence::number_sequences(&mut reader, count),
        type_id::COLOR_SEQUENCE => sequence::color_sequences(&mut reader, count),
        type_id::NUMBER_RANGE => sequence::number_ranges(&mut reader, count),
        type_id::RECT => vector::rects(&mut reader, count),
        type_id::PHYSICAL_PROPERTIES => scalar::physical_properties(&mut reader, count),
        type_id::COLOR3_UINT8 => scalar::color3_uint8s(&mut reader, count),
        type_id::INT64 => scalar::int64s(&mut reader, count),
        type_id::SHARED_STRING => scalar::shared_strings(&mut reader, count, shared),
        type_id::OPTIONAL_CFRAME => cframe::optional_cframes(&mut reader, count),
        type_id::UNIQUE_ID => identity::unique_ids(&mut reader, count),
        type_id::FONT => font::fonts(&mut reader, count),
        type_id::SECURITY_CAPABILITIES => scalar::security_capabilities(&mut reader, count),
        type_id::CONTENT => content::contents(&mut reader, count),
        _ => Err(BinaryError::UnsupportedPropertyType(header.type_id)),
    }
}

// Budget for the fallback below: a corrupt file can announce thousands of
// instances for a small payload, and cloning it once per instance would turn a
// broken property into an out-of-memory.
const UNSPLITTABLE_BUDGET: usize = 64 * 1024;

// The only documented type id left without a decoder is 0x1D, an internal
// "Bytecode" tag rbx-dom's spec does not describe. Every other value reaching
// this fallback is either a future/unknown type id or a non-UTF-8 String blob.
fn unsupported(type_id: u8, count: usize, payload: &[u8]) -> PropValues {
    let stride = if count > 0 && payload.len().is_multiple_of(count) {
        Some(payload.len() / count)
    } else {
        // Variable-width element we don't know how to walk: splitting would be a
        // guess, so every instance keeps the whole payload instead of nothing.
        None
    };
    let keep_whole_payload =
        stride.is_none() && count.saturating_mul(payload.len()) <= UNSPLITTABLE_BUDGET;

    (0..count)
        .map(|index| {
            let raw = match stride {
                Some(stride) => payload[index * stride..(index + 1) * stride].to_vec(),
                None if keep_whole_payload => payload.to_vec(),
                None => Vec::new(),
            };
            Some(Variant::Unknown { type_id, raw })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(type_id: u8, payload: &[u8]) -> PropHeader<'_> {
        PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        }
    }

    #[test]
    fn parses_header_and_keeps_payload() {
        let mut data = Vec::new();
        data.extend_from_slice(&3i32.to_le_bytes());
        data.extend_from_slice(&4i32.to_le_bytes());
        data.extend_from_slice(b"Name");
        data.push(0x01);
        data.extend_from_slice(&[0xFF, 0xEE]);

        let header = parse_header(&data).unwrap();

        assert_eq!(header.class_id, 3);
        assert_eq!(header.name, "Name");
        assert_eq!(header.type_id, 0x01);
        assert_eq!(header.payload, &[0xFF, 0xEE]);
    }

    #[test]
    fn unknown_type_id_splits_evenly_when_it_can() {
        let payload = [1u8, 2, 3, 4];
        let values = decode(&header(0xFE, &payload), 2, &[]);

        assert_eq!(
            values,
            vec![
                Some(Variant::Unknown {
                    type_id: 0xFE,
                    raw: vec![1, 2]
                }),
                Some(Variant::Unknown {
                    type_id: 0xFE,
                    raw: vec![3, 4]
                }),
            ]
        );
    }

    #[test]
    fn truncated_known_type_degrades_instead_of_failing() {
        // Two Int32 values announced, only one present.
        let values = decode(&header(type_id::INT32, &[0, 0, 0, 2]), 2, &[]);

        assert!(values.iter().all(|value| matches!(
            value,
            Some(Variant::Unknown {
                type_id: type_id::INT32,
                ..
            })
        )));
    }
}
