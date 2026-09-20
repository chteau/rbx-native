//! CFrame and OptionalCFrame encoding, the encode counterpart of `chunks::prop::cframe`.

use rbx_dom::rotation::{basic_rotation_id, IDENTITY, RAW_ROTATION_ID};
use rbx_dom::{CFrameData, Variant, Vector3Data};

use super::vector;
use crate::serialize::prop::map_dense;
use crate::serialize::writer::Writer;
use crate::serialize::SerializeError;

const CFRAME_TYPE_ID: u8 = 0x10;
const BOOL_TYPE_ID: u8 = 0x02;
const ABSENT_CFRAME: CFrameData = CFrameData {
    position: Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: IDENTITY,
};

pub(super) fn cframes(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let frames = map_dense(class, name, values, |v| match v {
        Variant::CFrame(f) => Some(*f),
        _ => None,
    })?;
    Ok(encode_frames(&frames))
}

// Absent values still occupy a full CFrame slot (Roblox writes the identity), so the
// array can never be shortened; only the trailing presence byte marks them absent.
pub(super) fn optional_cframes(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let opts = map_dense(class, name, values, |v| match v {
        Variant::OptionalCFrame(o) => Some(*o),
        _ => None,
    })?;
    let frames: Vec<CFrameData> = opts.iter().map(|o| o.unwrap_or(ABSENT_CFRAME)).collect();

    let mut writer = Writer::new();
    writer.u8(CFRAME_TYPE_ID);
    writer.bytes(&encode_frames(&frames));
    writer.u8(BOOL_TYPE_ID);
    for present in opts.iter().map(Option::is_some) {
        writer.u8(u8::from(present));
    }
    Ok(writer.into_bytes())
}

fn encode_frames(frames: &[CFrameData]) -> Vec<u8> {
    let mut writer = Writer::new();
    for frame in frames {
        writer.bytes(&encode_rotation(&frame.rotation));
    }
    let positions: Vec<Vector3Data> = frames.iter().map(|f| f.position).collect();
    writer.bytes(&vector::components(&positions));
    writer.into_bytes()
}

fn encode_rotation(matrix: &[f32; 9]) -> Vec<u8> {
    if let Some(id) = basic_rotation_id(matrix) {
        return vec![id];
    }

    let mut writer = Writer::new();
    writer.u8(RAW_ROTATION_ID);
    for &component in matrix {
        writer.f32(component);
    }
    writer.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};

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
    fn cframe_round_trips_a_compressed_and_a_raw_rotation() {
        let raw_rotation = [0.9, 0.1, 0.0, -0.1, 0.9, 0.0, 0.0, 0.0, 1.0];
        let values = vec![
            Some(Variant::CFrame(ABSENT_CFRAME)),
            Some(Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                rotation: raw_rotation,
            })),
        ];
        let payload = cframes("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x10, 2, &payload), values);
    }

    #[test]
    fn optional_cframe_round_trips_present_and_absent() {
        let values = vec![
            Some(Variant::OptionalCFrame(Some(ABSENT_CFRAME))),
            Some(Variant::OptionalCFrame(None)),
        ];
        let payload = optional_cframes("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x1E, 2, &payload), values);
    }
}
