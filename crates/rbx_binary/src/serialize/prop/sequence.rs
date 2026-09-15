//! Encoders for NumberSequence, ColorSequence and NumberRange, the encode counterpart
//! of `chunks::prop::sequence`. All three are sequential per instance with plain
//! little-endian floats: no interleaving, no bit rotation.

use rbx_dom::Variant;

use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

pub(super) fn number_sequences(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let sequences = map_dense(class, name, values, |v| match v {
        Variant::NumberSequence(s) => Some(s.clone()),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for sequence in sequences {
        writer.length(sequence.keypoints.len());
        for keypoint in sequence.keypoints {
            writer.f32(keypoint.time);
            writer.f32(keypoint.value);
            writer.f32(keypoint.envelope);
        }
    }
    Ok(writer.into_bytes())
}

pub(super) fn color_sequences(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let sequences = map_dense(class, name, values, |v| match v {
        Variant::ColorSequence(s) => Some(s.clone()),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for sequence in sequences {
        writer.length(sequence.keypoints.len());
        for keypoint in sequence.keypoints {
            writer.f32(keypoint.time);
            writer.f32(keypoint.color.r);
            writer.f32(keypoint.color.g);
            writer.f32(keypoint.color.b);
            writer.f32(keypoint.envelope);
        }
    }
    Ok(writer.into_bytes())
}

pub(super) fn number_ranges(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let ranges = map_dense(class, name, values, |v| match v {
        Variant::NumberRange(r) => Some(*r),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for range in ranges {
        writer.f32(range.min);
        writer.f32(range.max);
    }
    Ok(writer.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::{
        Color3Data, ColorSequence, ColorSequenceKeypoint, NumberRange, NumberSequence,
        NumberSequenceKeypoint,
    };

    fn decoded(type_id: u8, count: usize, payload: &[u8]) -> Vec<Option<Variant>> {
        let header = PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        };
        decode(&header, count, &[])
    }

    #[test]
    fn number_sequence_round_trips_two_keypoints() {
        let values = vec![Some(Variant::NumberSequence(NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: 0.0,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: 0.5,
                    envelope: 0.0,
                },
            ],
        }))];
        let payload = number_sequences("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x15, 1, &payload), values);
    }

    #[test]
    fn color_sequence_round_trips() {
        let white = Color3Data {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        };
        let values = vec![Some(Variant::ColorSequence(ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 0.0,
                color: white,
                envelope: 0.0,
            }],
        }))];
        let payload = color_sequences("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x16, 1, &payload), values);
    }

    #[test]
    fn number_range_round_trips() {
        let values = vec![Some(Variant::NumberRange(NumberRange {
            min: 0.9,
            max: 1.05,
        }))];
        let payload = number_ranges("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x17, 1, &payload), values);
    }
}
