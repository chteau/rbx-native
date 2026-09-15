//! UDim and UDim2 encoding, the encode counterpart of `chunks::prop::udim`.

use rbx_dom::Variant;

use crate::codec::{interleave_f32, interleave_zigzag_i32};
use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

pub(super) fn udims(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let dims = map_dense(class, name, values, |v| match v {
        Variant::UDim(d) => Some(*d),
        _ => None,
    })?;

    let scales: Vec<f32> = dims.iter().map(|d| d.scale).collect();
    let offsets: Vec<i32> = dims.iter().map(|d| d.offset).collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_f32(&scales));
    writer.bytes(&interleave_zigzag_i32(&offsets));
    Ok(writer.into_bytes())
}

// Block order groups by kind, not by axis: x.scale, y.scale, x.offset, y.offset.
pub(super) fn udim2s(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let dims = map_dense(class, name, values, |v| match v {
        Variant::UDim2(d) => Some(*d),
        _ => None,
    })?;

    let x_scales: Vec<f32> = dims.iter().map(|d| d.x.scale).collect();
    let y_scales: Vec<f32> = dims.iter().map(|d| d.y.scale).collect();
    let x_offsets: Vec<i32> = dims.iter().map(|d| d.x.offset).collect();
    let y_offsets: Vec<i32> = dims.iter().map(|d| d.y.offset).collect();

    let mut writer = Writer::new();
    writer.bytes(&interleave_f32(&x_scales));
    writer.bytes(&interleave_f32(&y_scales));
    writer.bytes(&interleave_zigzag_i32(&x_offsets));
    writer.bytes(&interleave_zigzag_i32(&y_offsets));
    Ok(writer.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};
    use rbx_dom::{UDim, UDim2};

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
    fn udim_round_trips() {
        let values = vec![Some(Variant::UDim(UDim {
            scale: 0.5,
            offset: -12,
        }))];
        let payload = udims("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x06, 1, &payload), values);
    }

    #[test]
    fn udim2_round_trips() {
        let values = vec![Some(Variant::UDim2(UDim2 {
            x: UDim {
                scale: 0.0,
                offset: 100,
            },
            y: UDim {
                scale: 1.0,
                offset: -100,
            },
        }))];
        let payload = udim2s("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x07, 1, &payload), values);
    }
}
