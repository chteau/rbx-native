//! Encoders for multi-component vector property types, the encode counterpart of
//! `chunks::prop::vector`.

use rbx_dom::{Variant, Vector2Data, Vector3Data};

use crate::codec::interleave_f32;
use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

pub(super) fn vector2s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let points = map_dense(class, name, values, |v| match v {
        Variant::Vector2(p) => Some(*p),
        _ => None,
    })?;
    let mut writer = Writer::new();
    writer.bytes(&interleave_f32(&xs(&points)));
    writer.bytes(&interleave_f32(&ys(&points)));
    Ok(writer.into_bytes())
}

pub(super) fn vector3s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let points = map_dense(class, name, values, |v| match v {
        Variant::Vector3(p) => Some(*p),
        _ => None,
    })?;
    Ok(components(&points))
}

pub(super) fn color3s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let colors = map_dense(class, name, values, |v| match v {
        Variant::Color3(c) => Some(*c),
        _ => None,
    })?;
    let points: Vec<Vector3Data> = colors
        .iter()
        .map(|c| Vector3Data {
            x: c.r,
            y: c.g,
            z: c.b,
        })
        .collect();
    Ok(components(&points))
}

pub(super) fn rects(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let rects = map_dense(class, name, values, |v| match v {
        Variant::Rect(r) => Some(*r),
        _ => None,
    })?;
    let mins: Vec<Vector2Data> = rects.iter().map(|r| r.min).collect();
    let maxes: Vec<Vector2Data> = rects.iter().map(|r| r.max).collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_f32(&xs(&mins)));
    writer.bytes(&interleave_f32(&ys(&mins)));
    writer.bytes(&interleave_f32(&xs(&maxes)));
    writer.bytes(&interleave_f32(&ys(&maxes)));
    Ok(writer.into_bytes())
}

/// Writes three interleaved component blocks (X, Y, Z), the encode counterpart of
/// `chunks::prop::vector::components`. Shared by Vector3, Color3 and CFrame positions.
pub(super) fn components(points: &[Vector3Data]) -> Vec<u8> {
    let xs: Vec<f32> = points.iter().map(|p| p.x).collect();
    let ys: Vec<f32> = points.iter().map(|p| p.y).collect();
    let zs: Vec<f32> = points.iter().map(|p| p.z).collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_f32(&xs));
    writer.bytes(&interleave_f32(&ys));
    writer.bytes(&interleave_f32(&zs));
    writer.into_bytes()
}

fn xs(points: &[Vector2Data]) -> Vec<f32> {
    points.iter().map(|p| p.x).collect()
}

fn ys(points: &[Vector2Data]) -> Vec<f32> {
    points.iter().map(|p| p.y).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::{Color3Data, Rect};

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
    fn vector3_round_trips_two_points() {
        let values = vec![
            Some(Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            })),
            Some(Variant::Vector3(Vector3Data {
                x: -4.0,
                y: 0.0,
                z: 6.5,
            })),
        ];
        let payload = vector3s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x0E, 2, &payload), values);
    }

    #[test]
    fn color3_round_trips() {
        let values = vec![Some(Variant::Color3(Color3Data {
            r: 0.25,
            g: 0.5,
            b: 0.75,
        }))];
        let payload = color3s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x0C, 1, &payload), values);
    }

    #[test]
    fn vector2_round_trips() {
        let values = vec![Some(Variant::Vector2(Vector2Data { x: -1.5, y: 2.5 }))];
        let payload = vector2s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x0D, 1, &payload), values);
    }

    #[test]
    fn rect_round_trips_in_min_max_order() {
        let values = vec![Some(Variant::Rect(Rect {
            min: Vector2Data { x: -1.0, y: -10.0 },
            max: Vector2Data { x: 8.0, y: 9.0 },
        }))];
        let payload = rects("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x18, 1, &payload), values);
    }
}
