//! Decoders for the remaining sequential, untransformed types: Ray, Faces, Axes,
//! Vector3int16. Unlike Vector2/Vector3/Color3, these are stored one full record
//! per instance (no column interleaving, no bit rotation, no zigzag).

use rbx_dom::{Axes, Faces, Variant, Vector3Data};

use super::PropValues;
use crate::codec::Reader;
use crate::error::BinaryError;

/// Reads a Ray property array.
///
/// Stored sequentially (no interleaving): one origin Vector3, then one direction Vector3, per instance.
pub(super) fn rays(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            let origin = vector3(reader)?;
            let direction = vector3(reader)?;
            Ok(Some(Variant::Ray { origin, direction }))
        })
        .collect()
}

fn vector3(reader: &mut Reader<'_>) -> Result<Vector3Data, BinaryError> {
    Ok(Vector3Data {
        x: reader.f32()?,
        y: reader.f32()?,
        z: reader.f32()?,
    })
}

/// Reads a Faces property array.
///
/// One byte per instance: bitfield in low 6 bits (Front, Bottom, Left, Back, Top, Right).
pub(super) fn faces(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| Ok(Some(Variant::Faces(Faces::from_bits(reader.u8()?)))))
        .collect()
}

/// Reads an Axes property array.
///
/// One byte per instance: bitfield in low 3 bits (X, Y, Z).
pub(super) fn axes(reader: &mut Reader<'_>, count: usize) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| Ok(Some(Variant::Axes(Axes::from_bits(reader.u8()?)))))
        .collect()
}

/// Reads a Vector3int16 property array.
///
/// Stored sequentially (no interleaving): three i16 values per instance.
pub(super) fn vector3int16s(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<PropValues, BinaryError> {
    (0..count)
        .map(|_| {
            Ok(Some(Variant::Vector3int16 {
                x: reader.i16()?,
                y: reader.i16()?,
                z: reader.i16()?,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32le(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn ray_reads_origin_then_direction_sequentially_for_each_instance() {
        let mut payload = f32le(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        payload.extend(f32le(&[0.0, 0.0, 0.0, -1.0, 0.0, 0.0]));

        let values = rays(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Ray {
                origin: Vector3Data {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                direction: Vector3Data {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0
                },
            })
        );
        assert_eq!(
            values[1],
            Some(Variant::Ray {
                origin: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                direction: Vector3Data {
                    x: -1.0,
                    y: 0.0,
                    z: 0.0
                },
            })
        );
    }

    #[test]
    fn ray_truncated_mid_record_is_an_error() {
        // One full Ray needs 24 bytes; only 20 are present.
        let payload = f32le(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(rays(&mut Reader::new(&payload), 1).is_err());
    }

    #[test]
    fn faces_reads_one_byte_per_instance_in_order() {
        // Front(bit0)+Right(bit5), then Bottom(bit1) alone.
        let values = faces(&mut Reader::new(&[0b0010_0001, 0b0000_0010]), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Faces(Faces {
                front: true,
                bottom: false,
                left: false,
                back: false,
                top: false,
                right: true,
            }))
        );
        assert_eq!(
            values[1],
            Some(Variant::Faces(Faces {
                front: false,
                bottom: true,
                left: false,
                back: false,
                top: false,
                right: false,
            }))
        );
    }

    #[test]
    fn faces_truncated_payload_is_an_error() {
        assert!(faces(&mut Reader::new(&[]), 1).is_err());
    }

    #[test]
    fn axes_reads_one_byte_per_instance_in_order() {
        // X+Z, then Y alone.
        let values = axes(&mut Reader::new(&[0b101, 0b010]), 2).unwrap();

        assert_eq!(
            values[0],
            Some(Variant::Axes(Axes {
                x: true,
                y: false,
                z: true
            }))
        );
        assert_eq!(
            values[1],
            Some(Variant::Axes(Axes {
                x: false,
                y: true,
                z: false
            }))
        );
    }

    #[test]
    fn axes_truncated_payload_is_an_error() {
        assert!(axes(&mut Reader::new(&[]), 1).is_err());
    }

    #[test]
    fn vector3int16_reads_three_i16_per_instance_sequentially() {
        let mut payload = Vec::new();
        for v in [1i16, -2, 3, 100, -200, 300] {
            payload.extend_from_slice(&v.to_le_bytes());
        }

        let values = vector3int16s(&mut Reader::new(&payload), 2).unwrap();

        assert_eq!(values[0], Some(Variant::Vector3int16 { x: 1, y: -2, z: 3 }));
        assert_eq!(
            values[1],
            Some(Variant::Vector3int16 {
                x: 100,
                y: -200,
                z: 300
            })
        );
    }

    #[test]
    fn vector3int16_truncated_mid_record_is_an_error() {
        // One full record needs 6 bytes; only 4 are present.
        let payload = [0u8, 0, 0, 0];
        assert!(vector3int16s(&mut Reader::new(&payload), 1).is_err());
    }
}
