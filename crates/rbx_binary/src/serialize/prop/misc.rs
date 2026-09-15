//! Encoders for the untransformed sequential types (Ray, Faces, Axes, Vector3int16)
//! and the generic `Unknown` blob fallback.

use rbx_dom::{Axes, Faces, Variant, Vector3Data};

use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

pub(super) fn rays(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let rays = map_dense(class, name, values, |v| match v {
        Variant::Ray { origin, direction } => Some((*origin, *direction)),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for (origin, direction) in rays {
        write_vector3(&mut writer, origin);
        write_vector3(&mut writer, direction);
    }
    Ok(writer.into_bytes())
}

fn write_vector3(writer: &mut Writer, v: Vector3Data) {
    writer.f32(v.x);
    writer.f32(v.y);
    writer.f32(v.z);
}

pub(super) fn faces(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let faces = map_dense(class, name, values, |v| match v {
        Variant::Faces(f) => Some(*f),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for f in faces {
        writer.u8(faces_bits(f));
    }
    Ok(writer.into_bytes())
}

fn faces_bits(f: Faces) -> u8 {
    (f.front as u8)
        | (f.bottom as u8) << 1
        | (f.left as u8) << 2
        | (f.back as u8) << 3
        | (f.top as u8) << 4
        | (f.right as u8) << 5
}

pub(super) fn axes(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let axes = map_dense(class, name, values, |v| match v {
        Variant::Axes(a) => Some(*a),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for a in axes {
        writer.u8(axes_bits(a));
    }
    Ok(writer.into_bytes())
}

fn axes_bits(a: Axes) -> u8 {
    (a.x as u8) | (a.y as u8) << 1 | (a.z as u8) << 2
}

pub(super) fn vector3int16s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let points = map_dense(class, name, values, |v| match v {
        Variant::Vector3int16 { x, y, z } => Some((*x, *y, *z)),
        _ => None,
    })?;

    let mut writer = Writer::new();
    for (x, y, z) in points {
        writer.i16(x);
        writer.i16(y);
        writer.i16(z);
    }
    Ok(writer.into_bytes())
}

// Generic fallback for a wire type id this crate never decodes into a typed `Variant`
// (documented example: 0x1D, an internal "Bytecode" tag). Requires every instance's raw
// blob to share one byte length, matching the reader's only lossless split (a fixed
// stride); different lengths are unrepresentable through that same fallback and are
// rejected rather than silently corrupted.
pub(super) fn unknown_blob(
    class: &str,
    name: &str,
    type_id: u8,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let raws: Vec<&[u8]> = values
        .iter()
        .map(|value| match value {
            Some(Variant::Unknown { type_id: t, raw }) if *t == type_id => Ok(raw.as_slice()),
            _ => Err(SerializeError::mismatch(class, name)),
        })
        .collect::<Result<_, _>>()?;

    let Some(&first) = raws.first() else {
        return Ok(Vec::new());
    };
    if raws.iter().any(|raw| raw.len() != first.len()) {
        return Err(SerializeError::VariableLengthUnknown {
            class: class.to_owned(),
            property: name.to_owned(),
        });
    }

    Ok(raws.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use crate::serialize::SerializeError;

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
    fn ray_round_trips_origin_then_direction() {
        let values = vec![Some(Variant::Ray {
            origin: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            direction: Vector3Data {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            },
        })];
        let payload = rays("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x08, 1, &payload), values);
    }

    #[test]
    fn faces_and_axes_round_trip() {
        let face_values = vec![Some(Variant::Faces(Faces {
            front: true,
            bottom: false,
            left: true,
            back: false,
            top: true,
            right: false,
        }))];
        let payload = faces("Test", "Prop", &face_values).unwrap();
        assert_eq!(decoded(0x09, 1, &payload), face_values);

        let axes_values = vec![Some(Variant::Axes(Axes {
            x: true,
            y: false,
            z: true,
        }))];
        let payload = axes("Test", "Prop", &axes_values).unwrap();
        assert_eq!(decoded(0x0A, 1, &payload), axes_values);
    }

    #[test]
    fn vector3int16_round_trips_negative_components() {
        let values = vec![Some(Variant::Vector3int16 {
            x: -1,
            y: 2,
            z: -300,
        })];
        let payload = vector3int16s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x14, 1, &payload), values);
    }

    #[test]
    fn unknown_blob_round_trips_a_uniform_stride() {
        let values = vec![
            Some(Variant::Unknown {
                type_id: 0x1D,
                raw: vec![1, 2, 3, 4],
            }),
            Some(Variant::Unknown {
                type_id: 0x1D,
                raw: vec![5, 6, 7, 8],
            }),
        ];
        let payload = unknown_blob("Test", "Prop", 0x1D, &values).unwrap();
        assert_eq!(decoded(0x1D, 2, &payload), values);
    }

    #[test]
    fn unknown_blob_rejects_mismatched_lengths() {
        let values = vec![
            Some(Variant::Unknown {
                type_id: 0x1D,
                raw: vec![1, 2],
            }),
            Some(Variant::Unknown {
                type_id: 0x1D,
                raw: vec![3, 4, 5],
            }),
        ];
        let err = unknown_blob("Test", "Prop", 0x1D, &values).unwrap_err();
        assert!(matches!(err, SerializeError::VariableLengthUnknown { .. }));
    }
}
