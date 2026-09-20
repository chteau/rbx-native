//! CFrame (coordinate frame) property decoding.
//!
//! CFrames combine a position (Vector3) with a 3×3 rotation matrix. The format
//! compresses axis-aligned rotations to a single byte, storing only raw matrices
//! when the rotation is not axis-aligned.

use rbx_dom::rotation::{basic_rotation, IDENTITY, RAW_ROTATION_ID};
use rbx_dom::{CFrameData, Variant, Vector3Data};

use super::{vector, PropValues};
use crate::codec::Reader;
use crate::error::BinaryError;

// Type ids the OptionalCFrame payload repeats inline; see `optional_cframes`.
const CFRAME_TYPE_ID: u8 = 0x10;
const BOOL_TYPE_ID: u8 = 0x02;

/// Reads and decodes a CFrame property array.
///
/// CFrame arrays are stored in two sections: first, one rotation per instance (a marker byte,
/// followed by either a 9-float raw matrix or nothing for compressed axis-aligned rotations),
/// then a single Vector3 array holding all positions.
pub(super) fn cframes(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    Ok(frames(reader, count)?
        .into_iter()
        .map(|frame| Some(Variant::CFrame(frame)))
        .collect())
}

/// Reads and decodes an OptionalCFrame property array.
///
/// The payload is a complete CFrame array sandwiched between two inline type ids:
/// `0x10` (CFrame) opens it and `0x02` (Bool) introduces a trailing presence byte
/// per instance. Absent values are still written as a full CFrame — Roblox emits the
/// identity — so the array cannot be shortened and the bytes must be consumed.
pub(super) fn optional_cframes(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    expect_type_id(reader, CFRAME_TYPE_ID)?;
    let frames = frames(reader, count)?;
    expect_type_id(reader, BOOL_TYPE_ID)?;

    frames
        .into_iter()
        .map(|frame| {
            Ok(Some(Variant::OptionalCFrame(
                present(reader)?.then_some(frame),
            )))
        })
        .collect()
}

// The inline ids are redundant with the property's own type id, so a mismatch means
// the layout is not the one we decode and the fallback should take over.
fn expect_type_id(reader: &mut Reader<'_>, expected: u8) -> Result<(), BinaryError> {
    let actual = reader.u8()?;
    if actual != expected {
        return Err(BinaryError::UnexpectedInnerType { expected, actual });
    }
    Ok(())
}

fn present(reader: &mut Reader<'_>) -> Result<bool, BinaryError> {
    Ok(reader.u8()? != 0)
}

fn frames(reader: &mut Reader<'_>, count: usize) -> Result<Vec<CFrameData>, BinaryError> {
    let rotations = (0..count)
        .map(|_| rotation(reader))
        .collect::<Result<Vec<_>, _>>()?;
    let positions = vector::components(reader, count)?;

    Ok((0..count)
        .map(|i| CFrameData {
            position: Vector3Data {
                x: positions[0][i],
                y: positions[1][i],
                z: positions[2][i],
            },
            rotation: rotations[i],
        })
        .collect())
}

/// Reads one CFrame rotation matrix, either compressed (axis-aligned) or raw (9 floats).
fn rotation(reader: &mut Reader<'_>) -> Result<[f32; 9], BinaryError> {
    let id = reader.u8()?;
    if id != RAW_ROTATION_ID {
        // An unknown id would mean a rotation we cannot reconstruct; the identity
        // keeps the position usable instead of failing the whole property.
        return Ok(basic_rotation(id).unwrap_or(IDENTITY));
    }

    let mut matrix = [0.0f32; 9];
    for component in matrix.iter_mut() {
        // Unlike every other float in the format, these nine are untransformed
        // IEEE-754 little-endian values.
        *component = reader.f32()?;
    }
    Ok(matrix)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_marker_reads_nine_untransformed_floats() {
        let mut payload = vec![RAW_ROTATION_ID];
        for component in IDENTITY {
            payload.extend_from_slice(&component.to_le_bytes());
        }
        // One position (0, 0, 0) as three interleaved blocks.
        payload.extend_from_slice(&[0; 12]);

        let values = cframes(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                rotation: IDENTITY,
            }))
        );
    }

    const ORIGIN: CFrameData = CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: IDENTITY,
    };

    // Workspace.WorldPivotData from TestPlace.rbxl, byte for byte: the 16 bytes
    // are 0x10, rotation id 2, twelve position bytes, 0x02, then the presence byte.
    #[test]
    fn optional_cframe_reads_the_real_workspace_pivot() {
        let payload = [0x10, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x01];

        let values = optional_cframes(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(values[0], Some(Variant::OptionalCFrame(Some(ORIGIN))));
    }

    // Tool.WorldPivotData from FPS.rbxm: two instances, neither with a value, yet
    // both still carry a full identity CFrame ahead of their 0x00 presence byte.
    #[test]
    fn optional_cframe_reads_two_real_absent_pivots() {
        let mut payload = vec![0x10, 0x02, 0x02];
        payload.extend_from_slice(&[0; 24]);
        payload.extend_from_slice(&[0x02, 0x00, 0x00]);
        assert_eq!(payload.len(), 30);

        let values = optional_cframes(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values,
            vec![
                Some(Variant::OptionalCFrame(None)),
                Some(Variant::OptionalCFrame(None)),
            ]
        );
    }

    #[test]
    fn a_missing_inner_type_id_is_rejected() {
        let payload = [0x11, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02, 0x01];

        assert!(matches!(
            optional_cframes(&mut Reader::new(&payload), 1),
            Err(BinaryError::UnexpectedInnerType {
                expected: 0x10,
                actual: 0x11
            })
        ));
    }
}
