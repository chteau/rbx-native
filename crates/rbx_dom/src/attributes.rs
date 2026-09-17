//! Decoding of the attribute blob Roblox packs into a single string property.
//!
//! `AttributesSerialize` (instance attributes) and `StyleRule`'s
//! `PropertiesSerialize` share one format: a `u32` count followed by that many
//! `name`/`type id`/`value` triples, every scalar little-endian. It is kept as
//! an opaque [`Variant::String`]/[`Variant::Unknown`] in the DOM and decoded
//! here on demand, so a file round-trips byte for byte whether or not this
//! module understands every type in it.
//!
//! The type ids are the attribute format's own and do not line up with the
//! binary format's property type ids — `0x0F` is a `Color3` here and a `Ref`
//! there.

use std::collections::BTreeMap;

use crate::variant::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, Font, FontStyle, NumberRange, NumberSequence,
    NumberSequenceKeypoint, Rect, UDim, UDim2, Variant, Vector2Data, Vector3Data,
};

/// Every attribute in `value`, or an empty map where there are none.
///
/// A blob this cannot read decodes to nothing rather than to a partial map:
/// the entries are variable-length and packed end to end, so one unknown type
/// id leaves no way to find where the next name begins.
pub fn decode(value: Option<&Variant>) -> BTreeMap<String, Variant> {
    let bytes = match value {
        Some(Variant::String(text)) => text.as_bytes(),
        Some(Variant::Unknown { raw, .. }) => raw.as_slice(),
        _ => return BTreeMap::new(),
    };
    read_all(&mut Cursor(bytes)).unwrap_or_default()
}

fn read_all(cursor: &mut Cursor<'_>) -> Option<BTreeMap<String, Variant>> {
    let mut attributes = BTreeMap::new();
    // An empty property is the common case: no attributes were ever set.
    if cursor.0.is_empty() {
        return Some(attributes);
    }
    for _ in 0..cursor.u32()? {
        let name = cursor.string()?;
        let value = cursor.value()?;
        attributes.insert(name, value);
    }
    Some(attributes)
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<String> {
        let length = self.u32()? as usize;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }

    fn udim(&mut self) -> Option<UDim> {
        Some(UDim {
            scale: self.f32()?,
            offset: self.i32()?,
        })
    }

    fn vector2(&mut self) -> Option<Vector2Data> {
        Some(Vector2Data {
            x: self.f32()?,
            y: self.f32()?,
        })
    }

    fn color3(&mut self) -> Option<Color3Data> {
        Some(Color3Data {
            r: self.f32()?,
            g: self.f32()?,
            b: self.f32()?,
        })
    }

    /// One value, its type read from the leading id byte.
    ///
    /// `CFrame` (`0x14`) is the one documented type left out: no GUI property
    /// takes one, and its rotation is either nine floats or an id into a table
    /// of axis-aligned bases, which is a table this crate has no other use for.
    fn value(&mut self) -> Option<Variant> {
        Some(match self.u8()? {
            0x02 => Variant::String(self.string()?),
            0x03 => Variant::Bool(self.u8()? != 0),
            0x04 => Variant::Int32(self.i32()?),
            0x05 => Variant::Float32(self.f32()?),
            0x06 => Variant::Float64(self.f64()?),
            0x09 => Variant::UDim(self.udim()?),
            0x0A => Variant::UDim2(UDim2 {
                x: self.udim()?,
                y: self.udim()?,
            }),
            0x0E => Variant::BrickColor(self.u32()?),
            0x0F => Variant::Color3(self.color3()?),
            0x10 => Variant::Vector2(self.vector2()?),
            0x11 => Variant::Vector3(Vector3Data {
                x: self.f32()?,
                y: self.f32()?,
                z: self.f32()?,
            }),
            // The enum's type name is written out beside the ordinal; only the
            // ordinal survives, which is all `Variant::Enum` holds anywhere
            // else in this crate.
            0x15 => {
                let _ty = self.string()?;
                Variant::Enum(self.u32()?)
            }
            0x17 => {
                let count = self.u32()?;
                let mut keypoints = Vec::with_capacity(count.min(1024) as usize);
                for _ in 0..count {
                    keypoints.push(NumberSequenceKeypoint {
                        envelope: self.f32()?,
                        time: self.f32()?,
                        value: self.f32()?,
                    });
                }
                Variant::NumberSequence(NumberSequence { keypoints })
            }
            0x19 => {
                let count = self.u32()?;
                let mut keypoints = Vec::with_capacity(count.min(1024) as usize);
                for _ in 0..count {
                    // The envelope of a `ColorSequenceKeypoint` is always zero;
                    // it is written all the same.
                    keypoints.push(ColorSequenceKeypoint {
                        envelope: self.f32()?,
                        time: self.f32()?,
                        color: self.color3()?,
                    });
                }
                Variant::ColorSequence(ColorSequence { keypoints })
            }
            0x1B => Variant::NumberRange(NumberRange {
                min: self.f32()?,
                max: self.f32()?,
            }),
            0x1C => Variant::Rect(Rect {
                min: self.vector2()?,
                max: self.vector2()?,
            }),
            0x21 => {
                let weight = self.u16()?;
                let style = self.u8()?;
                let family = self.string()?;
                let cached = self.string()?;
                Variant::Font(Font {
                    family,
                    weight,
                    style: FontStyle::from(style),
                    cached_face_id: (!cached.is_empty()).then_some(cached),
                })
            }
            _ => return None,
        })
    }
}

/// The `CollectionService` tags of an instance: the `Tags` property is the tag
/// names run together, separated by NUL bytes.
pub fn tags(value: Option<&Variant>) -> Vec<&str> {
    let text = match value {
        Some(Variant::String(text)) => text.as_str(),
        _ => return Vec::new(),
    };
    text.split('\0').filter(|tag| !tag.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `StyleRule.PropertiesSerialize` exactly as Roblox Studio wrote it into
    /// `rojo-rbx/rbx-test-files`' `models/stylesheet` sample, for a rule whose
    /// only property is `Size = UDim2.new(1, 0, 0.5, 0)`.
    const REAL_STYLE_RULE: &[u8] = &[
        0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x53, 0x69, 0x7a, 0x65, 0x0a, 0x00, 0x00,
        0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00,
    ];

    fn blob(bytes: &[u8]) -> Variant {
        Variant::Unknown {
            type_id: 0x01,
            raw: bytes.to_vec(),
        }
    }

    /// One entry named `name` holding `value`, with the count header.
    fn one(name: &str, value: &[u8]) -> Vec<u8> {
        let mut bytes = vec![1, 0, 0, 0];
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend(value);
        bytes
    }

    #[test]
    fn a_style_rule_written_by_studio_decodes_to_its_one_property() {
        let decoded = decode(Some(&blob(REAL_STYLE_RULE)));

        assert_eq!(
            decoded.get("Size"),
            Some(&Variant::UDim2(UDim2 {
                x: UDim {
                    scale: 1.0,
                    offset: 0
                },
                y: UDim {
                    scale: 0.5,
                    offset: 0
                },
            }))
        );
    }

    #[test]
    fn every_type_a_gui_property_can_hold_decodes() {
        let cases: Vec<(&[u8], Variant)> = vec![
            (&[0x03, 0x01], Variant::Bool(true)),
            (&[0x04, 0x07, 0x00, 0x00, 0x00], Variant::Int32(7)),
            (&[0x05, 0x00, 0x00, 0x80, 0x3f], Variant::Float32(1.0)),
            (&[0x06, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f], Variant::Float64(1.0)),
            (
                &[0x09, 0x00, 0x00, 0x00, 0x3f, 0x0a, 0x00, 0x00, 0x00],
                Variant::UDim(UDim {
                    scale: 0.5,
                    offset: 10,
                }),
            ),
            (
                &[0x0f, 0, 0, 0x80, 0x3f, 0, 0, 0, 0, 0, 0, 0, 0],
                Variant::Color3(Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                }),
            ),
            (
                &[0x10, 0, 0, 0, 0x3f, 0, 0, 0x80, 0x3f],
                Variant::Vector2(Vector2Data { x: 0.5, y: 1.0 }),
            ),
            (
                &[0x1b, 0, 0, 0, 0, 0, 0, 0x80, 0x3f],
                Variant::NumberRange(NumberRange { min: 0.0, max: 1.0 }),
            ),
            (&[0x0e, 0x18, 0x01, 0x00, 0x00], Variant::BrickColor(280)),
            // An `EnumItem` writes its enum's name before the ordinal.
            (
                &[
                    0x15, 0x04, 0x00, 0x00, 0x00, b'F', b'o', b'n', b't', 0x03, 0, 0, 0,
                ],
                Variant::Enum(3),
            ),
            (
                &[0x02, 0x02, 0x00, 0x00, 0x00, b'h', b'i'],
                Variant::String("hi".into()),
            ),
        ];

        for (encoded, expected) in cases {
            assert_eq!(
                decode(Some(&blob(&one("Token", encoded)))).get("Token"),
                Some(&expected),
                "decoding {encoded:02x?}"
            );
        }
    }

    #[test]
    fn a_font_keeps_its_family_weight_and_style() {
        let mut value = vec![0x21, 0x90, 0x01, 0x01];
        value.extend(4u32.to_le_bytes());
        value.extend(b"Sans");
        value.extend(0u32.to_le_bytes());

        assert_eq!(
            decode(Some(&blob(&one("Face", &value)))).get("Face"),
            Some(&Variant::Font(Font {
                family: "Sans".into(),
                weight: 400,
                style: FontStyle::Italic,
                cached_face_id: None,
            }))
        );
    }

    #[test]
    fn an_empty_or_absent_blob_is_no_attributes() {
        assert!(decode(None).is_empty());
        assert!(decode(Some(&Variant::String(String::new()))).is_empty());
    }

    #[test]
    fn a_type_this_cannot_read_takes_the_whole_blob_with_it() {
        // 0x14 is `CFrame`, deliberately unimplemented: the entries that
        // follow it can no longer be found.
        let mut bytes = vec![2, 0, 0, 0];
        bytes.extend(4u32.to_le_bytes());
        bytes.extend(b"Here");
        bytes.extend([0x03, 0x01]);
        bytes.extend(4u32.to_le_bytes());
        bytes.extend(b"Gone");
        bytes.push(0x14);

        assert!(decode(Some(&blob(&bytes))).is_empty());
    }

    #[test]
    fn a_truncated_blob_decodes_to_nothing() {
        assert!(decode(Some(&blob(&REAL_STYLE_RULE[..20]))).is_empty());
    }

    #[test]
    fn tags_are_split_on_nul_and_never_empty() {
        let value = Variant::String("Container\0BlueOnHover\0".into());

        assert_eq!(tags(Some(&value)), ["Container", "BlueOnHover"]);
        assert!(tags(Some(&Variant::String(String::new()))).is_empty());
        assert!(tags(None).is_empty());
    }
}
