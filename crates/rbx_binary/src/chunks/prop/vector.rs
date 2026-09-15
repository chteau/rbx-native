//! Decoders for multi-component vector property types (Vector2, Vector3, Color3).

use rbx_dom::{Color3Data, Rect, Variant, Vector2Data, Vector3Data};

use super::PropValues;
use crate::codec::Reader;
use crate::error::BinaryError;

// Multi-component float types are stored component-major: one complete
// interleaved block per component (every X, then every Y, ...), not one block of
// packed structs.
/// Decodes a Vector2 property array.
pub(super) fn vector2s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let xs = reader.interleaved_f32(count)?;
    let ys = reader.interleaved_f32(count)?;

    Ok((0..count)
        .map(|i| Some(Variant::Vector2(Vector2Data { x: xs[i], y: ys[i] })))
        .collect())
}

/// Decodes a Vector3 property array.
pub(super) fn vector3s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let components = components(reader, count)?;

    Ok((0..count)
        .map(|i| {
            Some(Variant::Vector3(Vector3Data {
                x: components[0][i],
                y: components[1][i],
                z: components[2][i],
            }))
        })
        .collect())
}

/// Decodes a Color3 property array.
pub(super) fn color3s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let components = components(reader, count)?;

    Ok((0..count)
        .map(|i| {
            Some(Variant::Color3(Color3Data {
                r: components[0][i],
                g: components[1][i],
                b: components[2][i],
            }))
        })
        .collect())
}

/// Decodes a Rect property array.
///
/// Four interleaved float blocks in the order Min.X, Min.Y, Max.X, Max.Y: the two
/// corners are *not* stored as two consecutive Vector2 arrays.
pub(super) fn rects(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let min_xs = reader.interleaved_f32(count)?;
    let min_ys = reader.interleaved_f32(count)?;
    let max_xs = reader.interleaved_f32(count)?;
    let max_ys = reader.interleaved_f32(count)?;

    Ok((0..count)
        .map(|i| {
            Some(Variant::Rect(Rect {
                min: Vector2Data {
                    x: min_xs[i],
                    y: min_ys[i],
                },
                max: Vector2Data {
                    x: max_xs[i],
                    y: max_ys[i],
                },
            }))
        })
        .collect())
}

/// Reads three components (X, Y, Z) of an interleaved float array.
///
/// Shared by Vector3 and Color3, and reused by the CFrame position array.
pub(super) fn components(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<[Vec<f32>; 3], BinaryError> {
    Ok([
        reader.interleaved_f32(count)?,
        reader.interleaved_f32(count)?,
        reader.interleaved_f32(count)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(values: &[f32]) -> Vec<u8> {
        // Mirror of the encoder: rotate the bits left, then store big-endian.
        let mut columns = vec![Vec::new(); 4];
        for value in values {
            let bytes = value.to_bits().rotate_left(1).to_be_bytes();
            for (column, byte) in columns.iter_mut().zip(bytes) {
                column.push(byte);
            }
        }
        columns.concat()
    }

    #[test]
    fn vector3_reads_one_block_per_component() {
        let mut payload = encoded(&[1.0, 2.0]);
        payload.extend(encoded(&[3.0, 4.0]));
        payload.extend(encoded(&[5.0, 6.0]));

        let values = vector3s(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 3.0,
                z: 5.0
            }))
        );
        assert_eq!(
            values[1],
            Some(Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 4.0,
                z: 6.0
            }))
        );
    }

    #[test]
    fn color3_shares_the_component_layout() {
        let mut payload = encoded(&[0.25]);
        payload.extend(encoded(&[0.5]));
        payload.extend(encoded(&[0.75]));

        let values = color3s(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Color3(Color3Data {
                r: 0.25,
                g: 0.5,
                b: 0.75
            }))
        );
    }

    #[test]
    fn vector2_is_two_blocks() {
        let mut payload = encoded(&[-1.5]);
        payload.extend(encoded(&[2.5]));

        let values = vector2s(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Vector2(Vector2Data { x: -1.5, y: 2.5 }))
        );
    }

    // ImageLabel.SliceCenter from TestPlace.rbxl is the all-zero default; this
    // adds the ordering the fixture cannot prove.
    #[test]
    fn rect_reads_four_blocks_in_min_max_order() {
        let mut payload = encoded(&[-1.0]);
        payload.extend(encoded(&[-10.0]));
        payload.extend(encoded(&[8.0]));
        payload.extend(encoded(&[9.0]));

        let values = rects(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Rect(Rect {
                min: Vector2Data { x: -1.0, y: -10.0 },
                max: Vector2Data { x: 8.0, y: 9.0 },
            }))
        );
    }
}
