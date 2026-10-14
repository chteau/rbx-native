//! Roblox property values on the wire, both directions: the probe encodes
//! what it reads, the editor decodes it into an `rbx_dom::Variant`, and the
//! editor encodes an edited `Variant` for `CommandBody::Set`. The probe's
//! Luau (`probe/codec.luau`) implements the same table.
//!
//! JSON shape by Roblox type (`t` tag, `v` payload):
//!
//! | Roblox type | JSON |
//! |---|---|
//! | bool / string / number | plain JSON value |
//! | number NaN, ±inf | `{"t":"n","v":"nan"\|"inf"\|"-inf"}` |
//! | Vector2 | `{"t":"V2","v":[x,y]}` |
//! | Vector3 | `{"t":"V3","v":[x,y,z]}` |
//! | Vector3int16 | `{"t":"V3i","v":[x,y,z]}` |
//! | CFrame | `{"t":"CF","v":[x,y,z,r00,r01,r02,r10,r11,r12,r20,r21,r22]}` (`GetComponents` order) |
//! | Color3 | `{"t":"C3","v":[r,g,b]}` (0..1) |
//! | BrickColor | `{"t":"BC","v":number}` |
//! | UDim | `{"t":"U","v":[scale,offset]}` |
//! | UDim2 | `{"t":"U2","v":[xs,xo,ys,yo]}` |
//! | Rect | `{"t":"R","v":[minx,miny,maxx,maxy]}` |
//! | NumberRange | `{"t":"NR","v":[min,max]}` |
//! | NumberSequence | `{"t":"NS","v":[[time,value,envelope],…]}` |
//! | ColorSequence | `{"t":"CS","v":[[time,r,g,b],…]}` |
//! | EnumItem | `{"t":"E","v":value,"n":"Name"}` |
//! | Instance | `{"t":"I","v":id}` (`0` = nil, or not in the tree) |
//! | Font | `{"t":"F","v":[family,weight,"Normal"\|"Italic"]}` |
//! | PhysicalProperties | `{"t":"PP","v":[density,friction,elasticity,frictionWeight,elasticityWeight]}`, `{"t":"PP"}` = default |
//! | Faces | `{"t":"Fa","v":["Top","Bottom","Left","Right","Back","Front"]}` (those set) |
//! | Axes | `{"t":"Ax","v":["X","Y","Z"]}` (those set) |
//! | Ray | `{"t":"Ray","v":[ox,oy,oz,dx,dy,dz]}` |
//! | Content | `{"t":"Ct","v":uri}` (`""` = none) |
//! | anything else | `{"t":"?","v":tostring(value)}` (read-only in the editor) |
//! | nil | `null` |
//!
//! Decoding is guided by the property's reflected `value_type` (an
//! `rbx_reflection::PropertyDescriptor::value_type`, e.g. `"float"`,
//! `"int64"`, `"Material"` for an enum, `"BasePart"` for a reference), so
//! a JSON number becomes `Float32`, `Float64`, `Int32` or `Int64` as the
//! property is declared, and `Color3` stays `Color3`.

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    FontStyle, NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, Ref,
    UDim, UDim2, Variant, Vector2Data, Vector3Data,
};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

/// A JSON number, or the `{"t":"n"}` tag for NaN and infinities.
fn num(json: &Value) -> Option<f64> {
    match json {
        Value::Number(n) => n.as_f64(),
        Value::Object(map) if map.get("t")?.as_str()? == "n" => match map.get("v")?.as_str()? {
            "nan" => Some(f64::NAN),
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

fn enc_num(n: f64) -> Value {
    if n.is_nan() {
        json!({"t": "n", "v": "nan"})
    } else if n.is_infinite() {
        json!({"t": "n", "v": if n > 0.0 { "inf" } else { "-inf" }})
    } else {
        Value::from(n)
    }
}

fn enc_nums(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|n| enc_num(*n)).collect())
}

fn tagged(tag: &str, v: Value) -> Value {
    json!({"t": tag, "v": v})
}

fn int(json: &Value) -> Option<i64> {
    match json {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                .map(|f| f.round() as i64)
        }),
        _ => None,
    }
}

fn floats(v: &Value, len: usize) -> Option<Vec<f64>> {
    let items = v.as_array()?;
    if items.len() != len {
        return None;
    }
    items.iter().map(num).collect()
}

fn rows(v: &Value, len: usize) -> Option<Vec<Vec<f64>>> {
    v.as_array()?.iter().map(|row| floats(row, len)).collect()
}

fn v3(a: &[f64]) -> Vector3Data {
    Vector3Data {
        x: a[0] as f32,
        y: a[1] as f32,
        z: a[2] as f32,
    }
}

fn i16_of(n: f64) -> Option<i16> {
    (n.is_finite()).then(|| n.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16)
}

fn decode_tagged(json: &Value) -> Option<Variant> {
    let map = json.as_object()?;
    let tag = map.get("t")?.as_str()?;
    let v = map.get("v");
    if tag == "PP" {
        return Some(Variant::PhysicalProperties(match v {
            None | Some(Value::Null) => PhysicalProperties::Default,
            Some(v) => {
                let a = floats(v, 5)?;
                PhysicalProperties::Custom {
                    density: a[0] as f32,
                    friction: a[1] as f32,
                    elasticity: a[2] as f32,
                    friction_weight: a[3] as f32,
                    elasticity_weight: a[4] as f32,
                }
            }
        }));
    }
    let v = v?;
    Some(match tag {
        "n" => Variant::Float64(num(json)?),
        "V2" => {
            let a = floats(v, 2)?;
            Variant::Vector2(Vector2Data {
                x: a[0] as f32,
                y: a[1] as f32,
            })
        }
        "V3" => Variant::Vector3(v3(&floats(v, 3)?)),
        "V3i" => {
            let a = floats(v, 3)?;
            Variant::Vector3int16 {
                x: i16_of(a[0])?,
                y: i16_of(a[1])?,
                z: i16_of(a[2])?,
            }
        }
        "CF" => {
            let a = floats(v, 12)?;
            let mut rotation = [0f32; 9];
            for (slot, value) in rotation.iter_mut().zip(&a[3..]) {
                *slot = *value as f32;
            }
            Variant::CFrame(CFrameData {
                position: v3(&a),
                rotation,
            })
        }
        "C3" => {
            let a = floats(v, 3)?;
            Variant::Color3(Color3Data {
                r: a[0] as f32,
                g: a[1] as f32,
                b: a[2] as f32,
            })
        }
        "BC" => {
            let n = num(v)?;
            (n.is_finite() && n >= 0.0).then_some(Variant::BrickColor(n as u32))?
        }
        "U" => {
            let a = floats(v, 2)?;
            Variant::UDim(udim(a[0], a[1])?)
        }
        "U2" => {
            let a = floats(v, 4)?;
            Variant::UDim2(UDim2 {
                x: udim(a[0], a[1])?,
                y: udim(a[2], a[3])?,
            })
        }
        "R" => {
            let a = floats(v, 4)?;
            Variant::Rect(Rect {
                min: Vector2Data {
                    x: a[0] as f32,
                    y: a[1] as f32,
                },
                max: Vector2Data {
                    x: a[2] as f32,
                    y: a[3] as f32,
                },
            })
        }
        "NR" => {
            let a = floats(v, 2)?;
            Variant::NumberRange(NumberRange {
                min: a[0] as f32,
                max: a[1] as f32,
            })
        }
        "NS" => Variant::NumberSequence(NumberSequence {
            keypoints: rows(v, 3)?
                .iter()
                .map(|r| NumberSequenceKeypoint {
                    time: r[0] as f32,
                    value: r[1] as f32,
                    envelope: r[2] as f32,
                })
                .collect(),
        }),
        "CS" => Variant::ColorSequence(ColorSequence {
            keypoints: rows(v, 4)?
                .iter()
                .map(|r| ColorSequenceKeypoint {
                    time: r[0] as f32,
                    color: Color3Data {
                        r: r[1] as f32,
                        g: r[2] as f32,
                        b: r[3] as f32,
                    },
                    envelope: 0.0,
                })
                .collect(),
        }),
        "F" => {
            let items = v.as_array()?;
            if items.len() != 3 {
                return None;
            }
            let style = match items[2].as_str()? {
                "Normal" => FontStyle::Normal,
                "Italic" => FontStyle::Italic,
                _ => return None,
            };
            Variant::Font(Font {
                family: items[0].as_str()?.to_owned(),
                weight: u16::try_from(int(&items[1])?).ok()?,
                style,
                cached_face_id: None,
            })
        }
        "Fa" => {
            let mut faces = Faces::default();
            for name in v.as_array()? {
                match name.as_str()? {
                    "Top" => faces.top = true,
                    "Bottom" => faces.bottom = true,
                    "Left" => faces.left = true,
                    "Right" => faces.right = true,
                    "Back" => faces.back = true,
                    "Front" => faces.front = true,
                    _ => return None,
                }
            }
            Variant::Faces(faces)
        }
        "Ax" => {
            let mut axes = Axes::default();
            for name in v.as_array()? {
                match name.as_str()? {
                    "X" => axes.x = true,
                    "Y" => axes.y = true,
                    "Z" => axes.z = true,
                    _ => return None,
                }
            }
            Variant::Axes(axes)
        }
        "Ray" => {
            let a = floats(v, 6)?;
            Variant::Ray {
                origin: v3(&a[..3]),
                direction: v3(&a[3..]),
            }
        }
        "Ct" => Variant::Content(content(v.as_str()?)),
        _ => return None,
    })
}

fn udim(scale: f64, offset: f64) -> Option<UDim> {
    offset.is_finite().then(|| UDim {
        scale: scale as f32,
        offset: offset.round() as i32,
    })
}

fn content(uri: &str) -> Content {
    if uri.is_empty() {
        Content::None
    } else {
        Content::Uri(uri.to_owned())
    }
}

fn encode_tagged(value: &Variant) -> Option<Value> {
    let f = |x: f32| f64::from(x);
    Some(match value {
        Variant::Vector2(p) => tagged("V2", enc_nums(&[f(p.x), f(p.y)])),
        Variant::Vector3(p) => tagged("V3", enc_nums(&[f(p.x), f(p.y), f(p.z)])),
        Variant::Vector3int16 { x, y, z } => tagged(
            "V3i",
            enc_nums(&[f64::from(*x), f64::from(*y), f64::from(*z)]),
        ),
        Variant::CFrame(c) => {
            let mut a = vec![f(c.position.x), f(c.position.y), f(c.position.z)];
            a.extend(c.rotation.iter().map(|r| f(*r)));
            tagged("CF", enc_nums(&a))
        }
        Variant::Color3(c) => tagged("C3", enc_nums(&[f(c.r), f(c.g), f(c.b)])),
        Variant::BrickColor(n) => tagged("BC", Value::from(*n)),
        Variant::UDim(u) => tagged("U", enc_nums(&[f(u.scale), f64::from(u.offset)])),
        Variant::UDim2(u) => tagged(
            "U2",
            enc_nums(&[
                f(u.x.scale),
                f64::from(u.x.offset),
                f(u.y.scale),
                f64::from(u.y.offset),
            ]),
        ),
        Variant::Rect(r) => tagged(
            "R",
            enc_nums(&[f(r.min.x), f(r.min.y), f(r.max.x), f(r.max.y)]),
        ),
        Variant::NumberRange(r) => tagged("NR", enc_nums(&[f(r.min), f(r.max)])),
        Variant::NumberSequence(s) => tagged(
            "NS",
            Value::Array(
                s.keypoints
                    .iter()
                    .map(|k| enc_nums(&[f(k.time), f(k.value), f(k.envelope)]))
                    .collect(),
            ),
        ),
        Variant::ColorSequence(s) => tagged(
            "CS",
            Value::Array(
                s.keypoints
                    .iter()
                    .map(|k| enc_nums(&[f(k.time), f(k.color.r), f(k.color.g), f(k.color.b)]))
                    .collect(),
            ),
        ),
        Variant::Font(font) => {
            let style = match font.style {
                FontStyle::Normal => "Normal",
                FontStyle::Italic => "Italic",
                FontStyle::Other(_) => return None,
            };
            tagged("F", json!([font.family, font.weight, style]))
        }
        Variant::PhysicalProperties(PhysicalProperties::Default) => json!({"t": "PP"}),
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density,
            friction,
            elasticity,
            friction_weight,
            elasticity_weight,
        }) => tagged(
            "PP",
            enc_nums(&[
                f(*density),
                f(*friction),
                f(*elasticity),
                f(*friction_weight),
                f(*elasticity_weight),
            ]),
        ),
        Variant::Faces(x) => {
            let names = [
                (x.top, "Top"),
                (x.bottom, "Bottom"),
                (x.left, "Left"),
                (x.right, "Right"),
                (x.back, "Back"),
                (x.front, "Front"),
            ];
            tagged("Fa", names.iter().filter(|n| n.0).map(|n| n.1).collect())
        }
        Variant::Axes(x) => {
            let names = [(x.x, "X"), (x.y, "Y"), (x.z, "Z")];
            tagged("Ax", names.iter().filter(|n| n.0).map(|n| n.1).collect())
        }
        Variant::Ray { origin, direction } => tagged(
            "Ray",
            enc_nums(&[
                f(origin.x),
                f(origin.y),
                f(origin.z),
                f(direction.x),
                f(direction.y),
                f(direction.z),
            ]),
        ),
        Variant::Content(Content::None) => tagged("Ct", json!("")),
        Variant::Content(Content::Uri(uri)) => tagged("Ct", json!(uri)),
        _ => return None,
    })
}

/// JSON → `Variant` for a property declared as `value_type`. `instance`
/// maps a probe id to the live DOM's `Ref` (`None` → the property is
/// omitted rather than set to a dangling ref; id `0` → `None` here too).
/// `None` when the JSON does not fit the type.
pub fn decode(
    json: &Value,
    value_type: &str,
    db: &ReflectionDatabase,
    instance: &dyn Fn(u64) -> Option<Ref>,
) -> Option<Variant> {
    match value_type {
        "bool" => return json.as_bool().map(Variant::Bool),
        "float" => return num(json).map(|n| Variant::Float32(n as f32)),
        "double" => return num(json).map(Variant::Float64),
        "int" => return i32::try_from(int(json)?).ok().map(Variant::Int32),
        "int64" => return int(json).map(Variant::Int64),
        "string" | "ContentId" | "ProtectedString" | "BinaryString" => {
            return json.as_str().map(|s| Variant::String(s.to_owned()));
        }
        "Content" => {
            return match json {
                Value::String(s) => Some(Variant::Content(content(s))),
                _ => match decode_tagged(json)? {
                    v @ Variant::Content(_) => Some(v),
                    _ => None,
                },
            };
        }
        _ => {}
    }
    let is_enum_tag = json
        .get("t")
        .and_then(Value::as_str)
        .is_some_and(|t| t == "E");
    if db.enum_items(value_type).is_some() && (is_enum_tag || value_type != "Font") {
        if !is_enum_tag {
            return None;
        }
        let n = num(json.get("v")?)?;
        return (n.is_finite() && n >= 0.0 && n <= f64::from(u32::MAX))
            .then_some(Variant::Enum(n as u32));
    }
    if db.class(value_type).is_some() {
        let map = json.as_object()?;
        if map.get("t")?.as_str()? != "I" {
            return None;
        }
        let id = map.get("v")?.as_u64()?;
        if id == 0 {
            return None;
        }
        return instance(id).map(Variant::Ref);
    }
    decode_tagged(json)
}

/// `Variant` → JSON for `CommandBody::Set` on a property declared as
/// `value_type`; the inverse of `decode`. `id_of` maps a live `Ref` to the
/// probe id (`None` → `{"t":"I","v":0}`). `None` for a variant this wire
/// format cannot carry (e.g. `SharedString`, `Unknown`).
pub fn encode(
    value: &Variant,
    value_type: &str,
    db: &ReflectionDatabase,
    id_of: &dyn Fn(Ref) -> Option<u64>,
) -> Option<Value> {
    Some(match value {
        Variant::Bool(b) => Value::from(*b),
        Variant::String(s) => Value::from(s.as_str()),
        Variant::Int32(n) => Value::from(*n),
        Variant::Int64(n) => Value::from(*n),
        Variant::Float32(n) => enc_num(f32_to_f64(*n)),
        Variant::Float64(n) => enc_num(*n),
        Variant::Enum(n) => {
            let name = db.enum_name(value_type, *n).unwrap_or("");
            json!({"t": "E", "v": n, "n": name})
        }
        Variant::Ref(r) => tagged("I", Value::from(id_of(*r).unwrap_or(0))),
        other => encode_tagged(other)?,
    })
}

/// An attribute value (attributes carry their own type): JSON → `Variant`
/// using the tag alone — numbers become `Float64`, strings `String`, bools
/// `Bool`, and tagged values as in the table. `None` for `null`/unknown.
pub fn decode_attribute(json: &Value) -> Option<Variant> {
    match json {
        Value::Bool(b) => Some(Variant::Bool(*b)),
        Value::String(s) => Some(Variant::String(s.clone())),
        Value::Number(n) => n.as_f64().map(Variant::Float64),
        Value::Object(_) => decode_tagged(json),
        _ => None,
    }
}

/// The inverse of `decode_attribute`.
pub fn encode_attribute(value: &Variant) -> Option<Value> {
    Some(match value {
        Variant::Bool(b) => Value::from(*b),
        Variant::String(s) => Value::from(s.as_str()),
        Variant::Float64(n) => enc_num(*n),
        Variant::Float32(n) => enc_num(f32_to_f64(*n)),
        Variant::Int32(n) => Value::from(*n),
        Variant::Int64(n) => Value::from(*n),
        other => encode_tagged(other)?,
    })
}

/// A float32 widened through its shortest decimal form, so 0.8 goes out as
/// 0.8 rather than 0.800000011920929.
fn f32_to_f64(n: f32) -> f64 {
    n.to_string().parse().unwrap_or(f64::from(n))
}
