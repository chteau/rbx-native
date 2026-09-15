//! Decoders for the keyframed and ranged types: NumberSequence, ColorSequence, NumberRange.
//!
//! Unlike the numeric array types, these three are stored sequentially per instance
//! with plain little-endian IEEE-754 floats: no interleaving, no bit rotation. A
//! variable keypoint count makes a column layout impossible for the two sequences,
//! and Roblox writes NumberRange the same way for consistency.

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberRange, NumberSequence,
    NumberSequenceKeypoint, Variant,
};

use super::PropValues;
use crate::codec::Reader;
use crate::error::BinaryError;

/// Reads a NumberSequence property array.
///
/// The count field is attacker-controlled, so keypoints are read one at a time
/// and let the bounded reader fail rather than pre-allocating what it announces.
pub(super) fn number_sequences(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            let keypoints = (0..reader.length()?)
                .map(|_| {
                    Ok(NumberSequenceKeypoint {
                        time: reader.f32()?,
                        value: reader.f32()?,
                        envelope: reader.f32()?,
                    })
                })
                .collect::<Result<Vec<_>, BinaryError>>()?;

            Ok(Some(Variant::NumberSequence(NumberSequence { keypoints })))
        })
        .collect()
}

/// Reads a ColorSequence property array.
///
/// Each keypoint stores a time, color, and envelope (which is serialized but
/// has no effect in the engine; it is kept for round-trip preservation).
pub(super) fn color_sequences(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            let keypoints = (0..reader.length()?)
                .map(|_| {
                    Ok(ColorSequenceKeypoint {
                        time: reader.f32()?,
                        color: Color3Data {
                            r: reader.f32()?,
                            g: reader.f32()?,
                            b: reader.f32()?,
                        },
                        envelope: reader.f32()?,
                    })
                })
                .collect::<Result<Vec<_>, BinaryError>>()?;

            Ok(Some(Variant::ColorSequence(ColorSequence { keypoints })))
        })
        .collect()
}

/// Reads a NumberRange property array.
///
/// Stored as plain min/max pairs (no interleaving or encoding like other numeric types).
pub(super) fn number_ranges(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            Ok(Some(Variant::NumberRange(NumberRange {
                min: reader.f32()?,
                max: reader.f32()?,
            })))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len() / 2)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    // UIGradient.Transparency from TestPlace.rbxl: fully opaque from t=0 to t=1.
    #[test]
    fn number_sequence_reads_real_uigradient_transparency() {
        let payload = hex("020000000000000000000000000000000000803f0000000000000000");
        let values = number_sequences(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::NumberSequence(NumberSequence {
                keypoints: vec![
                    NumberSequenceKeypoint {
                        time: 0.0,
                        value: 0.0,
                        envelope: 0.0
                    },
                    NumberSequenceKeypoint {
                        time: 1.0,
                        value: 0.0,
                        envelope: 0.0
                    },
                ],
            }))
        );
    }

    // UIGradient.Color from TestPlace.rbxl: white at both ends.
    #[test]
    fn color_sequence_reads_real_uigradient_color() {
        let payload = hex(
            "02000000000000000000803f0000803f0000803f000000000000803f0000803f0000803f0000803f00000000",
        );
        let values = color_sequences(&mut Reader::new(&payload), 1).unwrap();

        let white = Color3Data {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        };
        assert_eq!(
            values[0],
            Some(Variant::ColorSequence(ColorSequence {
                keypoints: vec![
                    ColorSequenceKeypoint {
                        time: 0.0,
                        color: white,
                        envelope: 0.0
                    },
                    ColorSequenceKeypoint {
                        time: 1.0,
                        color: white,
                        envelope: 0.0
                    },
                ],
            }))
        );
    }

    // StarterPlayer.GameSettingsScaleRangeHeight from TestPlace.rbxl.
    #[test]
    fn number_range_reads_two_plain_floats() {
        let payload = hex("6666663f6666863f");
        let values = number_ranges(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::NumberRange(NumberRange {
                min: 0.9,
                max: 1.05
            }))
        );
    }

    #[test]
    fn an_announced_keypoint_count_the_payload_cannot_back_is_an_error() {
        let mut payload = 1_000_000i32.to_le_bytes().to_vec();
        payload.extend_from_slice(&[0; 12]);

        assert!(number_sequences(&mut Reader::new(&payload), 1).is_err());
    }
}
