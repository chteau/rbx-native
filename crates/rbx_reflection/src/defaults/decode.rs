//! Turning one of rbx-dom's JSON default values into a [`Variant`]: the
//! shapes the database writes each type in, and the `"inf"`/`"-inf"`
//! strings it cannot write as numbers.

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    FontStyle, NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, UDim,
    UDim2, Variant, Vector2Data, Vector3Data,
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Deserialize)]
struct RawCFrame {
    position: [f32; 3],
    /// Rows, the way the DOM's own row-major `rotation` lays them out.
    orientation: [[f32; 3]; 3],
}

#[derive(Deserialize)]
struct RawSequence<K> {
    keypoints: Vec<K>,
}

#[derive(Deserialize)]
struct RawNumberKeypoint {
    time: f32,
    value: f32,
    envelope: f32,
}

#[derive(Deserialize)]
struct RawColorKeypoint {
    time: f32,
    color: [f32; 3],
}

#[derive(Deserialize)]
struct RawFont {
    family: String,
    weight: String,
    style: String,
    #[serde(rename = "cachedFaceId")]
    cached_face_id: Option<String>,
}

#[derive(Deserialize)]
struct RawRay {
    origin: [f32; 3],
    direction: [f32; 3],
}

/// rbx-dom's own spelling of a value, one variant per type the generator
/// keeps (see `scripts/reflection-defaults.sh`).
#[derive(Deserialize)]
enum Raw {
    Bool(bool),
    Int32(i32),
    Int64(i64),
    Float32(Float),
    Float64(f64),
    String(String),
    Enum(u32),
    BrickColor(u32),
    SecurityCapabilities(u64),
    Color3([f32; 3]),
    Color3uint8([u8; 3]),
    Vector2([Float; 2]),
    Vector3([f32; 3]),
    Vector3int16([i16; 3]),
    CFrame(RawCFrame),
    OptionalCFrame(Option<RawCFrame>),
    UDim((f32, i32)),
    UDim2([(f32, i32); 2]),
    Rect([[f32; 2]; 2]),
    NumberRange([f32; 2]),
    NumberSequence(RawSequence<RawNumberKeypoint>),
    ColorSequence(RawSequence<RawColorKeypoint>),
    PhysicalProperties(Value),
    Font(RawFont),
    Faces(Vec<String>),
    Axes(Vec<String>),
    Ray(RawRay),
    Content(Value),
}

/// A float as the file spells it: a number, or `"inf"`/`"-inf"`, which JSON
/// has no number for (`AlignPosition.MaxVelocity`, `UISizeConstraint.MaxSize`).
struct Float(f32);

impl<'de> Deserialize<'de> for Float {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Spelled {
            Number(f32),
            Word(String),
        }
        match Spelled::deserialize(deserializer)? {
            Spelled::Number(value) => Ok(Float(value)),
            Spelled::Word(word) if word == "inf" => Ok(Float(f32::INFINITY)),
            Spelled::Word(word) if word == "-inf" => Ok(Float(f32::NEG_INFINITY)),
            Spelled::Word(word) => {
                Err(serde::de::Error::custom(format!("{word:?} is not a float")))
            }
        }
    }
}

pub(super) fn variant(value: Value, weight: &impl Fn(&str) -> Option<u16>) -> Option<Variant> {
    Some(match serde_json::from_value::<Raw>(value).ok()? {
        Raw::Bool(value) => Variant::Bool(value),
        Raw::Int32(value) => Variant::Int32(value),
        Raw::Int64(value) => Variant::Int64(value),
        Raw::Float32(Float(value)) => Variant::Float32(value),
        Raw::Float64(value) => Variant::Float64(value),
        Raw::String(value) => Variant::String(value),
        Raw::Enum(value) => Variant::Enum(value),
        Raw::BrickColor(value) => Variant::BrickColor(value),
        Raw::SecurityCapabilities(value) => Variant::SecurityCapabilities(value),
        Raw::Color3(rgb) => Variant::Color3(color3(rgb)),
        Raw::Color3uint8([r, g, b]) => Variant::Color3uint8 { r, g, b },
        Raw::Vector2(xy) => Variant::Vector2(vector2(xy.map(|Float(value)| value))),
        Raw::Vector3(xyz) => Variant::Vector3(vector3(xyz)),
        Raw::Vector3int16([x, y, z]) => Variant::Vector3int16 { x, y, z },
        Raw::CFrame(frame) => Variant::CFrame(cframe(frame)),
        Raw::OptionalCFrame(frame) => Variant::OptionalCFrame(frame.map(cframe)),
        Raw::UDim(udim) => Variant::UDim(self::udim(udim)),
        Raw::UDim2([x, y]) => Variant::UDim2(UDim2 {
            x: udim(x),
            y: udim(y),
        }),
        Raw::Rect([min, max]) => Variant::Rect(Rect {
            min: vector2(min),
            max: vector2(max),
        }),
        Raw::NumberRange([min, max]) => Variant::NumberRange(NumberRange { min, max }),
        Raw::NumberSequence(sequence) => Variant::NumberSequence(NumberSequence {
            keypoints: sequence
                .keypoints
                .into_iter()
                .map(|k| NumberSequenceKeypoint {
                    time: k.time,
                    value: k.value,
                    envelope: k.envelope,
                })
                .collect(),
        }),
        Raw::ColorSequence(sequence) => Variant::ColorSequence(ColorSequence {
            keypoints: sequence
                .keypoints
                .into_iter()
                .map(|k| ColorSequenceKeypoint {
                    time: k.time,
                    color: color3(k.color),
                    envelope: 0.0,
                })
                .collect(),
        }),
        // `Default` is the only form a new instance is ever created with;
        // anything else is left for the caller to go without.
        Raw::PhysicalProperties(Value::String(form)) if form == "Default" => {
            Variant::PhysicalProperties(PhysicalProperties::Default)
        }
        Raw::PhysicalProperties(_) => return None,
        Raw::Font(font) => Variant::Font(Font {
            family: font.family,
            weight: weight(&font.weight)?,
            style: match font.style.as_str() {
                "Normal" => FontStyle::Normal,
                "Italic" => FontStyle::Italic,
                _ => return None,
            },
            cached_face_id: font.cached_face_id.filter(|id| !id.is_empty()),
        }),
        Raw::Faces(names) => {
            let has = |face: &str| names.iter().any(|name| name == face);
            Variant::Faces(Faces {
                front: has("Front"),
                bottom: has("Bottom"),
                left: has("Left"),
                back: has("Back"),
                top: has("Top"),
                right: has("Right"),
            })
        }
        Raw::Axes(names) => {
            let has = |axis: &str| names.iter().any(|name| name == axis);
            Variant::Axes(Axes {
                x: has("X"),
                y: has("Y"),
                z: has("Z"),
            })
        }
        Raw::Ray(ray) => Variant::Ray {
            origin: vector3(ray.origin),
            direction: vector3(ray.direction),
        },
        Raw::Content(content) => Variant::Content(match content {
            Value::String(none) if none == "None" => Content::None,
            Value::Object(map) => match map.get("Uri") {
                Some(Value::String(uri)) => Content::Uri(uri.clone()),
                _ => return None,
            },
            _ => return None,
        }),
    })
}

fn color3([r, g, b]: [f32; 3]) -> Color3Data {
    Color3Data { r, g, b }
}

fn vector2([x, y]: [f32; 2]) -> Vector2Data {
    Vector2Data { x, y }
}

fn vector3([x, y, z]: [f32; 3]) -> Vector3Data {
    Vector3Data { x, y, z }
}

fn udim((scale, offset): (f32, i32)) -> UDim {
    UDim { scale, offset }
}

fn cframe(frame: RawCFrame) -> CFrameData {
    let [a, b, c] = frame.orientation;
    CFrameData {
        position: vector3(frame.position),
        rotation: [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]],
    }
}
