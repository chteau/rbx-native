//! Decoders for the UDim and UDim2 property types.

use rbx_dom::{UDim, UDim2, Variant};

use super::PropValues;
use crate::codec::{zigzag_i32, Reader};
use crate::error::BinaryError;

/// Reads a UDim property array.
///
/// Stores scale and offset as separate interleaved blocks: every scale first (rotated float),
/// then every offset (zigzag i32).
pub(super) fn udims(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let scales = reader.interleaved_f32(count)?;
    let offsets = offsets(reader, count)?;

    Ok(scales
        .into_iter()
        .zip(offsets)
        .map(|(scale, offset)| Some(Variant::UDim(UDim { scale, offset })))
        .collect())
}

/// Reads a UDim2 property array.
///
/// Block order groups by kind (all scales, then all offsets), not by axis:
/// x.scale, y.scale, x.offset, y.offset.
pub(super) fn udim2s(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    let x_scales = reader.interleaved_f32(count)?;
    let y_scales = reader.interleaved_f32(count)?;
    let x_offsets = offsets(reader, count)?;
    let y_offsets = offsets(reader, count)?;

    Ok((0..count)
        .map(|i| {
            Some(Variant::UDim2(UDim2 {
                x: UDim {
                    scale: x_scales[i],
                    offset: x_offsets[i],
                },
                y: UDim {
                    scale: y_scales[i],
                    offset: y_offsets[i],
                },
            }))
        })
        .collect())
}

fn offsets(reader: &mut Reader<'_>, count: usize) -> Result<Vec<i32>, BinaryError> {
    Ok(reader
        .interleaved_u32(count)?
        .into_iter()
        .map(zigzag_i32)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // UICorner.TopLeftRadius from TestPlace.rbxl: scale 0, offset 12 (zigzag 24).
    #[test]
    fn udim_reads_scale_then_offset_blocks() {
        let payload = [0, 0, 0, 0, 0, 0, 0, 0x18];
        let values = udims(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::UDim(UDim {
                scale: 0.0,
                offset: 12
            }))
        );
    }

    // ImageLabel.Size from TestPlace.rbxl: {{0, 100}, {0, 100}}.
    #[test]
    fn udim2_groups_scales_before_offsets() {
        let payload = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xC8, 0, 0, 0, 0xC8];
        let values = udim2s(&mut Reader::new(&payload), 1).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::UDim2(UDim2 {
                x: UDim {
                    scale: 0.0,
                    offset: 100
                },
                y: UDim {
                    scale: 0.0,
                    offset: 100
                },
            }))
        );
    }
}
