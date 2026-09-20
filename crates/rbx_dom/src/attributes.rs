//! The attribute blob Roblox packs into a single string property, read by
//! [`decode`] and written back by [`encode`].
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

use crate::rotation::{self, RAW_ROTATION_ID};
use crate::variant::{
    CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Font, FontStyle, NumberRange,
    NumberSequence, NumberSequenceKeypoint, Rect, UDim, UDim2, Variant, Vector2Data, Vector3Data,
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

    /// A `CFrame`: a position, then one rotation-id byte that is either an
    /// entry in [`rotation`]'s table of axis-aligned bases or `0`, meaning
    /// nine raw floats (the row-major matrix `CFrameData` holds) follow.
    fn cframe(&mut self) -> Option<CFrameData> {
        let position = Vector3Data {
            x: self.f32()?,
            y: self.f32()?,
            z: self.f32()?,
        };
        let rotation = match self.u8()? {
            RAW_ROTATION_ID => {
                let mut matrix = [0.0; 9];
                for component in &mut matrix {
                    *component = self.f32()?;
                }
                matrix
            }
            // An id outside the table leaves no way to know how many bytes
            // belong to this value, so the blob is unreadable from here on.
            id => rotation::basic_rotation(id)?,
        };
        Some(CFrameData { position, rotation })
    }

    /// One value, its type read from the leading id byte.
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
            0x14 => Variant::CFrame(self.cframe()?),
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

/// The blob [`decode`] would read back as `attributes`, or `None` when one of
/// the values has no type id here.
///
/// `enum_type` names the enum a [`Variant::Enum`] belongs to: the format
/// writes that name beside the ordinal and `Variant::Enum` only carries the
/// ordinal, so the caller — which knows the class the property sits on — has
/// to supply it. An enum whose name it cannot give is refused rather than
/// written with an empty one, since nothing here can say whether Roblox's own
/// reader needs it.
///
/// Every value is written in the same little-endian layout `decode`'s cursor
/// reads, so the pair round-trips byte for byte for a blob this module
/// understands whole.
pub fn encode(
    attributes: &BTreeMap<String, Variant>,
    enum_type: impl Fn(&str) -> Option<String>,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.extend((attributes.len() as u32).to_le_bytes());
    for (name, value) in attributes {
        string(&mut out, name);
        write(&mut out, value, || enum_type(name))?;
    }
    Some(out)
}

fn string(out: &mut Vec<u8>, text: &str) {
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(text.as_bytes());
}

fn udim(out: &mut Vec<u8>, value: &UDim) {
    out.extend(value.scale.to_le_bytes());
    out.extend(value.offset.to_le_bytes());
}

fn vector2(out: &mut Vec<u8>, value: &Vector2Data) {
    out.extend(value.x.to_le_bytes());
    out.extend(value.y.to_le_bytes());
}

fn color3(out: &mut Vec<u8>, value: &Color3Data) {
    out.extend(value.r.to_le_bytes());
    out.extend(value.g.to_le_bytes());
    out.extend(value.b.to_le_bytes());
}

/// One value, its type id first — the exact inverse of `Cursor::value`.
fn write(out: &mut Vec<u8>, value: &Variant, enum_type: impl Fn() -> Option<String>) -> Option<()> {
    match value {
        Variant::String(text) => {
            out.push(0x02);
            string(out, text);
        }
        Variant::Bool(flag) => out.extend([0x03, u8::from(*flag)]),
        Variant::Int32(number) => {
            out.push(0x04);
            out.extend(number.to_le_bytes());
        }
        Variant::Float32(number) => {
            out.push(0x05);
            out.extend(number.to_le_bytes());
        }
        Variant::Float64(number) => {
            out.push(0x06);
            out.extend(number.to_le_bytes());
        }
        Variant::UDim(value) => {
            out.push(0x09);
            udim(out, value);
        }
        Variant::UDim2(value) => {
            out.push(0x0A);
            udim(out, &value.x);
            udim(out, &value.y);
        }
        Variant::BrickColor(index) => {
            out.push(0x0E);
            out.extend(index.to_le_bytes());
        }
        Variant::Color3(value) => {
            out.push(0x0F);
            color3(out, value);
        }
        Variant::Vector2(value) => {
            out.push(0x10);
            vector2(out, value);
        }
        Variant::Vector3(value) => {
            out.push(0x11);
            out.extend(value.x.to_le_bytes());
            out.extend(value.y.to_le_bytes());
            out.extend(value.z.to_le_bytes());
        }
        Variant::CFrame(frame) => {
            out.push(0x14);
            out.extend(frame.position.x.to_le_bytes());
            out.extend(frame.position.y.to_le_bytes());
            out.extend(frame.position.z.to_le_bytes());
            // Studio writes the one-byte id whenever the rotation has one,
            // so re-encoding a file it wrote reproduces its bytes.
            match rotation::basic_rotation_id(&frame.rotation) {
                Some(id) => out.push(id),
                None => {
                    out.push(RAW_ROTATION_ID);
                    for component in frame.rotation {
                        out.extend(component.to_le_bytes());
                    }
                }
            }
        }
        Variant::Enum(ordinal) => {
            out.push(0x15);
            string(out, &enum_type()?);
            out.extend(ordinal.to_le_bytes());
        }
        Variant::NumberSequence(sequence) => {
            out.push(0x17);
            out.extend((sequence.keypoints.len() as u32).to_le_bytes());
            for keypoint in &sequence.keypoints {
                out.extend(keypoint.envelope.to_le_bytes());
                out.extend(keypoint.time.to_le_bytes());
                out.extend(keypoint.value.to_le_bytes());
            }
        }
        Variant::ColorSequence(sequence) => {
            out.push(0x19);
            out.extend((sequence.keypoints.len() as u32).to_le_bytes());
            for keypoint in &sequence.keypoints {
                out.extend(keypoint.envelope.to_le_bytes());
                out.extend(keypoint.time.to_le_bytes());
                color3(out, &keypoint.color);
            }
        }
        Variant::NumberRange(range) => {
            out.push(0x1B);
            out.extend(range.min.to_le_bytes());
            out.extend(range.max.to_le_bytes());
        }
        Variant::Rect(rect) => {
            out.push(0x1C);
            vector2(out, &rect.min);
            vector2(out, &rect.max);
        }
        Variant::Font(font) => {
            out.push(0x21);
            out.extend(font.weight.to_le_bytes());
            out.push(u8::from(font.style));
            string(out, &font.family);
            string(out, font.cached_face_id.as_deref().unwrap_or(""));
        }
        _ => return None,
    }
    Some(())
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

/// The `Tags` property text for `tags` — the exact inverse of [`tags`]: each
/// name followed by its own NUL, the same shape a Studio-written file already
/// round-trips through [`tags`] (see this module's tests). A tag holding a
/// NUL byte itself would read back as two, so the caller is expected to have
/// refused one before this is called — this function does not check, the
/// same way [`encode`] does not validate an attribute name.
pub fn encode_tags(tags: &[&str]) -> String {
    tags.iter().map(|tag| format!("{tag}\0")).collect()
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

    /// The two `CFrame` examples `rojo-rbx/rbx-dom`'s attribute format
    /// document prints byte for byte (`docs/attributes.md`): a bare
    /// translation, which takes the one-byte id `02`, and a 45 degree turn
    /// about Y, which takes the raw nine floats.
    const SPEC_TRANSLATED: &[u8] = &[
        0x14, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x40, 0x40, 0x02,
    ];
    const SPEC_TURNED: &[u8] = &[
        0x14, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x40, 0x40, 0x00, 0xf3,
        0x04, 0x35, 0x3f, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x04, 0x35, 0x3f, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x04, 0x35, 0xbf, 0x00, 0x00, 0x00,
        0x00, 0xf3, 0x04, 0x35, 0x3f,
    ];

    fn frame_at_1_2_3(rotation: [f32; 9]) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation,
        })
    }

    #[test]
    fn a_cframe_with_an_axis_aligned_rotation_reads_from_its_one_byte_id() {
        let decoded = decode(Some(&blob(&one("Pivot", SPEC_TRANSLATED))));
        assert_eq!(decoded["Pivot"], frame_at_1_2_3(rotation::IDENTITY));
    }

    #[test]
    fn a_cframe_with_a_free_rotation_reads_its_nine_floats() {
        let decoded = decode(Some(&blob(&one("Pivot", SPEC_TURNED))));
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(
            decoded["Pivot"],
            frame_at_1_2_3([s, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, s])
        );
    }

    #[test]
    fn a_cframe_re_encodes_to_the_bytes_the_spec_prints() {
        for spec in [SPEC_TRANSLATED, SPEC_TURNED] {
            let original = one("Pivot", spec);
            let attributes = decode(Some(&blob(&original)));
            let again = encode(&attributes, |_| None).expect("a CFrame attribute should encode");
            assert_eq!(again, original);
        }
    }

    #[test]
    fn a_cframe_no_longer_takes_the_attributes_after_it_with_it() {
        let mut bytes = vec![2, 0, 0, 0];
        bytes.extend(1u32.to_le_bytes());
        bytes.push(b'A');
        bytes.extend(SPEC_TRANSLATED);
        bytes.extend(1u32.to_le_bytes());
        bytes.push(b'B');
        bytes.extend([0x03, 0x01]);

        let decoded = decode(Some(&blob(&bytes)));
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded["B"], Variant::Bool(true));
    }

    #[test]
    fn a_cframe_with_a_rotation_id_outside_the_table_is_unreadable() {
        // 0x01 and 0x04 are among the 12 collisions the table rejects; the
        // reader cannot know the value's length, so it reads nothing.
        let mut bad = SPEC_TRANSLATED.to_vec();
        *bad.last_mut().unwrap() = 0x01;
        assert!(decode(Some(&blob(&one("Pivot", &bad)))).is_empty());
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
        // 0x7F is no type this format defines: the entries that follow it
        // can no longer be found.
        let mut bytes = vec![2, 0, 0, 0];
        bytes.extend(4u32.to_le_bytes());
        bytes.extend(b"Here");
        bytes.extend([0x03, 0x01]);
        bytes.extend(4u32.to_le_bytes());
        bytes.extend(b"Gone");
        bytes.push(0x7F);

        assert!(decode(Some(&blob(&bytes))).is_empty());
    }

    #[test]
    fn a_truncated_blob_decodes_to_nothing() {
        assert!(decode(Some(&blob(&REAL_STYLE_RULE[..20]))).is_empty());
    }

    /// `encode` is the inverse of `decode` for every type the cursor reads —
    /// the blob, not just the map, so a re-encoded property is byte for byte
    /// what Studio would have written.
    #[test]
    fn every_type_the_decoder_reads_round_trips_through_encode() {
        let attributes: BTreeMap<String, Variant> = [
            ("AString", Variant::String("hi".into())),
            ("Bool", Variant::Bool(true)),
            ("Int", Variant::Int32(-7)),
            ("Float", Variant::Float32(0.25)),
            ("Double", Variant::Float64(0.125)),
            (
                "Dim",
                Variant::UDim(UDim {
                    scale: 0.5,
                    offset: 10,
                }),
            ),
            (
                "Dim2",
                Variant::UDim2(UDim2 {
                    x: UDim {
                        scale: 1.0,
                        offset: 0,
                    },
                    y: UDim {
                        scale: 0.5,
                        offset: -4,
                    },
                }),
            ),
            ("Brick", Variant::BrickColor(280)),
            (
                "Colour",
                Variant::Color3(Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.5,
                }),
            ),
            ("Two", Variant::Vector2(Vector2Data { x: 0.5, y: 1.0 })),
            (
                "Three",
                Variant::Vector3(Vector3Data {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }),
            ),
            ("Member", Variant::Enum(3)),
            (
                "Numbers",
                Variant::NumberSequence(NumberSequence {
                    keypoints: vec![
                        NumberSequenceKeypoint {
                            envelope: 0.0,
                            time: 0.0,
                            value: 1.0,
                        },
                        NumberSequenceKeypoint {
                            envelope: 0.25,
                            time: 1.0,
                            value: 0.0,
                        },
                    ],
                }),
            ),
            (
                "Colours",
                Variant::ColorSequence(ColorSequence {
                    keypoints: vec![ColorSequenceKeypoint {
                        envelope: 0.0,
                        time: 0.5,
                        color: Color3Data {
                            r: 0.0,
                            g: 1.0,
                            b: 0.0,
                        },
                    }],
                }),
            ),
            (
                "Range",
                Variant::NumberRange(NumberRange { min: 0.0, max: 8.0 }),
            ),
            (
                "Box",
                Variant::Rect(Rect {
                    min: Vector2Data { x: 0.0, y: 1.0 },
                    max: Vector2Data { x: 2.0, y: 3.0 },
                }),
            ),
            (
                "Face",
                Variant::Font(Font {
                    family: "Sans".into(),
                    weight: 700,
                    style: FontStyle::Italic,
                    cached_face_id: Some("rbxasset://x".into()),
                }),
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();

        let bytes = encode(&attributes, |_| Some("Font".to_owned())).expect("encodable");

        assert_eq!(decode(Some(&blob(&bytes))), attributes);
    }

    /// The blob a re-encode writes is the same bytes Studio wrote, not just
    /// the same map — what makes writing one back to `PropertiesSerialize`
    /// safe.
    #[test]
    fn a_studio_written_rule_re_encodes_to_the_same_bytes() {
        let decoded = decode(Some(&blob(REAL_STYLE_RULE)));

        assert_eq!(encode(&decoded, |_| None).as_deref(), Some(REAL_STYLE_RULE));
    }

    /// `Variant::Enum` carries no enum name, so one the caller cannot name
    /// is refused rather than written with an empty name that Roblox's own
    /// reader may or may not accept.
    #[test]
    fn an_enum_with_no_type_name_is_refused() {
        let attributes = BTreeMap::from([("Member".to_owned(), Variant::Enum(1))]);

        assert_eq!(encode(&attributes, |_| None), None);
    }

    /// A value the cursor never produces (`decode` stops at an unknown type
    /// id) has no id to write either.
    #[test]
    fn a_type_the_decoder_cannot_read_cannot_be_encoded() {
        let attributes = BTreeMap::from([("Where".to_owned(), Variant::Ref(crate::Ref::new(1)))]);

        assert_eq!(encode(&attributes, |_| Some("Font".to_owned())), None);
    }

    #[test]
    fn tags_are_split_on_nul_and_never_empty() {
        let value = Variant::String("Container\0BlueOnHover\0".into());

        assert_eq!(tags(Some(&value)), ["Container", "BlueOnHover"]);
        assert!(tags(Some(&Variant::String(String::new()))).is_empty());
        assert!(tags(None).is_empty());
    }

    #[test]
    fn encode_tags_is_the_inverse_of_tags() {
        assert_eq!(
            encode_tags(&["Container", "BlueOnHover"]),
            "Container\0BlueOnHover\0"
        );
        assert_eq!(
            tags(Some(&Variant::String(encode_tags(&["Alone"])))),
            ["Alone"]
        );
        assert_eq!(encode_tags(&[]), "");
    }
}
