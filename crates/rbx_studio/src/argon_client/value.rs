//! `rbx_dom::Variant` ⇄ Argon's `EncodedValue` — a single-key MsgPack map
//! whose key is Roblox's own canonical DataType name (`{Vector3: [x,y,z]}`,
//! `{Enum: 256}`, ...), dispatched by tag exactly the way
//! `rbx_xml::value::mod` dispatches XML property elements by tag name (that
//! module's own doc comment is the precedent: no reflection database
//! needed, Argon's own encoders commit to these names for compatibility).
//!
//! One quirk `argon-roblox`'s hand-rolled MsgPack encoder has that a
//! from-scratch decoder must account for: a number is written as MsgPack
//! int or float purely by its *value*, never by the property's declared
//! type — a `Float64`-tagged whole number (say, `Part.Size.X == 4.0`)
//! arrives as a MsgPack integer. Every numeric decode below is therefore
//! int-or-float tolerant.

use rbx_dom::{
    CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, NumberRange,
    NumberSequence, NumberSequenceKeypoint, UDim, UDim2, Variant, Vector2Data, Vector3Data,
};
use rmpv::Value;

fn num(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_i64().map(|i| i as f64))
}

fn num_or_zero(value: &Value) -> f64 {
    num(value).unwrap_or(0.0)
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Boolean(b) => Some(*b),
        _ => None,
    }
}

/// One `EncodedValue`'s payload, keyed by `tag` (already split out of its
/// `{tag: payload}` wrapper by the caller — see [`decode`]).
fn decode_tagged(tag: &str, payload: &Value) -> Option<Variant> {
    Some(match tag {
        "String" => Variant::String(payload.as_str()?.to_owned()),
        "Bool" => Variant::Bool(as_bool(payload)?),
        "Int32" => Variant::Int32(num_or_zero(payload) as i32),
        "Int64" => Variant::Int64(num_or_zero(payload) as i64),
        "Float32" => Variant::Float32(num_or_zero(payload) as f32),
        "Float64" => Variant::Float64(num_or_zero(payload)),
        // A bare ordinal: the property's enum *name* isn't on the wire at
        // all for a canonical property (only for the generic-attribute
        // `EncodedValue.encodeNaive` path, which uses `EnumItem` instead
        // and this client doesn't consume attributes through yet).
        "Enum" => Variant::Enum(num_or_zero(payload) as u32),
        "BrickColor" => Variant::BrickColor(num_or_zero(payload) as u32),
        "Vector2" => {
            let a = array(payload);
            Variant::Vector2(Vector2Data {
                x: num_or_zero(a.first()?) as f32,
                y: num_or_zero(a.get(1)?) as f32,
            })
        }
        "Vector3" => {
            let a = array(payload);
            Variant::Vector3(Vector3Data {
                x: num_or_zero(a.first()?) as f32,
                y: num_or_zero(a.get(1)?) as f32,
                z: num_or_zero(a.get(2)?) as f32,
            })
        }
        "Color3" => {
            let a = array(payload);
            Variant::Color3(Color3Data {
                r: num_or_zero(a.first()?) as f32,
                g: num_or_zero(a.get(1)?) as f32,
                b: num_or_zero(a.get(2)?) as f32,
            })
        }
        "Color3uint8" => {
            let a = array(payload);
            Variant::Color3uint8 {
                r: num_or_zero(a.first()?) as u8,
                g: num_or_zero(a.get(1)?) as u8,
                b: num_or_zero(a.get(2)?) as u8,
            }
        }
        "UDim" => {
            let a = array(payload);
            Variant::UDim(UDim {
                scale: num_or_zero(a.first()?) as f32,
                offset: num_or_zero(a.get(1)?) as i32,
            })
        }
        "UDim2" => {
            let a = array(payload);
            let component = |pair: &Value| -> UDim {
                let c = array(pair);
                UDim {
                    scale: c.first().map(num_or_zero).unwrap_or(0.0) as f32,
                    offset: c.get(1).map(num_or_zero).unwrap_or(0.0) as i32,
                }
            };
            Variant::UDim2(UDim2 {
                x: component(a.first()?),
                y: component(a.get(1)?),
            })
        }
        "CFrame" => {
            let position = array(map_get(payload, "position")?);
            let orientation = array(map_get(payload, "orientation")?);
            let row = |i: usize| -> [f32; 3] {
                let r = orientation.get(i).map(array).unwrap_or(&[]);
                [
                    r.first().map(num_or_zero).unwrap_or(0.0) as f32,
                    r.get(1).map(num_or_zero).unwrap_or(0.0) as f32,
                    r.get(2).map(num_or_zero).unwrap_or(0.0) as f32,
                ]
            };
            let r0 = row(0);
            let r1 = row(1);
            let r2 = row(2);
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: position.first().map(num_or_zero).unwrap_or(0.0) as f32,
                    y: position.get(1).map(num_or_zero).unwrap_or(0.0) as f32,
                    z: position.get(2).map(num_or_zero).unwrap_or(0.0) as f32,
                },
                rotation: [
                    r0[0], r0[1], r0[2], r1[0], r1[1], r1[2], r2[0], r2[1], r2[2],
                ],
            })
        }
        "NumberRange" => {
            let a = array(payload);
            Variant::NumberRange(NumberRange {
                min: num_or_zero(a.first()?) as f32,
                max: num_or_zero(a.get(1)?) as f32,
            })
        }
        "NumberSequence" => {
            let keypoints = array(map_get(payload, "keypoints")?)
                .iter()
                .filter_map(|kp| {
                    Some(NumberSequenceKeypoint {
                        time: num_or_zero(map_get(kp, "time")?) as f32,
                        value: num_or_zero(map_get(kp, "value")?) as f32,
                        envelope: map_get(kp, "envelope").map(num_or_zero).unwrap_or(0.0) as f32,
                    })
                })
                .collect();
            Variant::NumberSequence(NumberSequence { keypoints })
        }
        "ColorSequence" => {
            let keypoints = array(map_get(payload, "keypoints")?)
                .iter()
                .filter_map(|kp| {
                    let c = array(map_get(kp, "color")?);
                    Some(ColorSequenceKeypoint {
                        time: num_or_zero(map_get(kp, "time")?) as f32,
                        color: Color3Data {
                            r: c.first().map(num_or_zero).unwrap_or(0.0) as f32,
                            g: c.get(1).map(num_or_zero).unwrap_or(0.0) as f32,
                            b: c.get(2).map(num_or_zero).unwrap_or(0.0) as f32,
                        },
                        envelope: map_get(kp, "envelope").map(num_or_zero).unwrap_or(0.0) as f32,
                    })
                })
                .collect();
            Variant::ColorSequence(ColorSequence { keypoints })
        }
        "Content" | "ContentId" => Variant::Content(if let Some(s) = payload.as_str() {
            match s {
                "None" => Content::None,
                uri => Content::Uri(uri.to_owned()),
            }
        } else if let Some(uri) = map_get(payload, "Uri").and_then(Value::as_str) {
            Content::Uri(uri.to_owned())
        } else {
            // `Object` (a Ref-valued Content) isn't implemented by Argon's
            // own plugin either — falls through to `None`.
            Content::None
        }),
        // Everything else (Font, PhysicalProperties, Axes/Faces,
        // SecurityCapabilities, UniqueId, Ray, Vector3int16, Rect2D, Ref)
        // falls here — kept rather than refused, the same "degrade, don't
        // drop the whole snapshot" choice `rbx_xml::value`'s own `_ =>
        // Variant::Unknown` fallback makes for a tag it doesn't recognize.
        _ => return None,
    })
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// Decodes one `EncodedValue` — a `{Tag: payload}` single-key map — into a
/// `Variant`. `None` for a tag this client doesn't (yet) decode, an empty
/// map (Argon's own "unoccupied optional," e.g. a `PhysicalProperties` left
/// as `"Default"`), or a payload shaped unlike what its tag promises.
pub(crate) fn decode(encoded: &Value) -> Option<Variant> {
    let (tag, payload) = encoded.as_map()?.first()?;
    decode_tagged(tag.as_str()?, payload)
}

/// The inverse of [`decode`], for write-back. Only the tags this client
/// actually decodes are encoded — a `Variant` this client read as `Unknown`
/// (because *decode* didn't recognize its tag) has nothing to round-trip
/// back into, and is skipped by the caller before reaching here.
pub(crate) fn encode(variant: &Variant) -> Option<Value> {
    let (tag, payload) = match variant {
        Variant::String(s) => ("String", Value::from(s.as_str())),
        Variant::Bool(b) => ("Bool", Value::from(*b)),
        Variant::Int32(i) => ("Int32", Value::from(*i)),
        Variant::Int64(i) => ("Int64", Value::from(*i)),
        Variant::Float32(f) => ("Float32", Value::from(*f)),
        Variant::Float64(f) => ("Float64", Value::from(*f)),
        Variant::Enum(e) => ("Enum", Value::from(*e)),
        Variant::BrickColor(c) => ("BrickColor", Value::from(*c)),
        Variant::Vector2(v) => (
            "Vector2",
            Value::Array(vec![Value::from(v.x), Value::from(v.y)]),
        ),
        Variant::Vector3(v) => (
            "Vector3",
            Value::Array(vec![Value::from(v.x), Value::from(v.y), Value::from(v.z)]),
        ),
        Variant::Color3(c) => (
            "Color3",
            Value::Array(vec![Value::from(c.r), Value::from(c.g), Value::from(c.b)]),
        ),
        Variant::Color3uint8 { r, g, b } => (
            "Color3uint8",
            Value::Array(vec![Value::from(*r), Value::from(*g), Value::from(*b)]),
        ),
        Variant::UDim(u) => (
            "UDim",
            Value::Array(vec![Value::from(u.scale), Value::from(u.offset)]),
        ),
        Variant::UDim2(u) => (
            "UDim2",
            Value::Array(vec![
                Value::Array(vec![Value::from(u.x.scale), Value::from(u.x.offset)]),
                Value::Array(vec![Value::from(u.y.scale), Value::from(u.y.offset)]),
            ]),
        ),
        Variant::CFrame(c) => (
            "CFrame",
            Value::Map(vec![
                (
                    Value::from("position"),
                    Value::Array(vec![
                        Value::from(c.position.x),
                        Value::from(c.position.y),
                        Value::from(c.position.z),
                    ]),
                ),
                (
                    Value::from("orientation"),
                    Value::Array(
                        c.rotation
                            .chunks(3)
                            .map(|row| Value::Array(row.iter().copied().map(Value::from).collect()))
                            .collect(),
                    ),
                ),
            ]),
        ),
        Variant::NumberRange(r) => (
            "NumberRange",
            Value::Array(vec![Value::from(r.min), Value::from(r.max)]),
        ),
        Variant::NumberSequence(seq) => (
            "NumberSequence",
            Value::Map(vec![(
                Value::from("keypoints"),
                Value::Array(
                    seq.keypoints
                        .iter()
                        .map(|kp| {
                            Value::Map(vec![
                                (Value::from("time"), Value::from(kp.time)),
                                (Value::from("value"), Value::from(kp.value)),
                                (Value::from("envelope"), Value::from(kp.envelope)),
                            ])
                        })
                        .collect(),
                ),
            )]),
        ),
        Variant::ColorSequence(seq) => (
            "ColorSequence",
            Value::Map(vec![(
                Value::from("keypoints"),
                Value::Array(
                    seq.keypoints
                        .iter()
                        .map(|kp| {
                            Value::Map(vec![
                                (Value::from("time"), Value::from(kp.time)),
                                (
                                    Value::from("color"),
                                    Value::Array(vec![
                                        Value::from(kp.color.r),
                                        Value::from(kp.color.g),
                                        Value::from(kp.color.b),
                                    ]),
                                ),
                                (Value::from("envelope"), Value::from(kp.envelope)),
                            ])
                        })
                        .collect(),
                ),
            )]),
        ),
        Variant::Content(content) => (
            "Content",
            match content {
                Content::None => Value::from("None"),
                Content::Uri(uri) => {
                    Value::Map(vec![(Value::from("Uri"), Value::from(uri.as_str()))])
                }
                // Not round-trippable — Argon's own encoder errors on this
                // too (`Object serializing is not currently implemented`).
                Content::Object(_) => return None,
            },
        ),
        _ => return None,
    };
    Some(Value::Map(vec![(Value::from(tag), payload)]))
}

#[cfg(test)]
mod tests;
