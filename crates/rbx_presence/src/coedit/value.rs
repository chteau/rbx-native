//! Property values as JSON, `{"t": "<Type>", "v": …}`, readable by the
//! Studio plugin. Types and shapes:
//! String "s" | Bool b | Int32 n | Int64 n | Float32 n | Float64 n |
//! BrickColor n (palette number) | Color3 [r,g,b] | Color3uint8 [r,g,b] |
//! Vector2 [x,y] | Vector3 [x,y,z] | Vector3int16 [x,y,z] | Ray [ox,oy,oz,dx,dy,dz] |
//! Faces [front,bottom,left,back,top,right] (bools) | Axes [x,y,z] (bools) |
//! CFrame [x,y,z,r00..r22] (12, `CFrame:GetComponents()` order) | OptionalCFrame same or null |
//! Enum n (ordinal) | Ref Target-object or null | NumberSequence [[t,v,e],…] |
//! ColorSequence [[t,r,g,b,e],…] | NumberRange [min,max] | Rect [minx,miny,maxx,maxy] |
//! PhysicalProperties "Default" or [density,friction,elasticity,frictionWeight,elasticityWeight] |
//! UDim [scale,offset] | UDim2 [xs,xo,ys,yo] | Font {"family","weight","style"} (style ordinal) |
//! SecurityCapabilities n | Content: see `encode`.
//! `UniqueId`, `SharedString` and `Unknown` are not sent (identity, a file-local
//! table index, and bytes we cannot interpret).

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, Ref, UDim,
    UDim2, Variant, Vector2Data, Vector3Data,
};
use serde_json::{json, Value};

use super::Target;

fn num(f: f32) -> Option<Value> {
    serde_json::Number::from_f64(f64::from(f)).map(Value::Number)
}

fn nums(fs: &[f32]) -> Option<Value> {
    fs.iter()
        .map(|&f| num(f))
        .collect::<Option<Vec<_>>>()
        .map(Value::Array)
}

fn cframe_nums(c: &CFrameData) -> Option<Value> {
    let mut all = vec![c.position.x, c.position.y, c.position.z];
    all.extend_from_slice(&c.rotation);
    nums(&all)
}

/// `None` for a type that is not sent. `target` names a `Ref`'s referent.
pub fn encode(value: &Variant, target: &dyn Fn(Ref) -> Option<Target>) -> Option<Value> {
    let (t, v) = match value {
        Variant::String(s) => ("String", json!(s)),
        Variant::Bool(b) => ("Bool", json!(b)),
        Variant::Int32(n) => ("Int32", json!(n)),
        Variant::Int64(n) => ("Int64", json!(n)),
        Variant::Float32(f) => ("Float32", num(*f)?),
        Variant::Float64(f) => (
            "Float64",
            serde_json::Number::from_f64(*f).map(Value::Number)?,
        ),
        Variant::BrickColor(n) => ("BrickColor", json!(n)),
        Variant::Color3(c) => ("Color3", nums(&[c.r, c.g, c.b])?),
        Variant::Color3uint8 { r, g, b } => ("Color3uint8", json!([r, g, b])),
        Variant::Vector2(p) => ("Vector2", nums(&[p.x, p.y])?),
        Variant::Vector3(p) => ("Vector3", nums(&[p.x, p.y, p.z])?),
        Variant::Vector3int16 { x, y, z } => ("Vector3int16", json!([x, y, z])),
        Variant::Ray { origin, direction } => (
            "Ray",
            nums(&[
                origin.x,
                origin.y,
                origin.z,
                direction.x,
                direction.y,
                direction.z,
            ])?,
        ),
        Variant::Faces(f) => (
            "Faces",
            json!([f.front, f.bottom, f.left, f.back, f.top, f.right]),
        ),
        Variant::Axes(a) => ("Axes", json!([a.x, a.y, a.z])),
        Variant::CFrame(c) => ("CFrame", cframe_nums(c)?),
        Variant::OptionalCFrame(c) => (
            "OptionalCFrame",
            match c {
                Some(c) => cframe_nums(c)?,
                None => Value::Null,
            },
        ),
        Variant::Enum(n) => ("Enum", json!(n)),
        Variant::Ref(r) => ("Ref", serde_json::to_value(target(*r)?).ok()?),
        Variant::NumberSequence(s) => (
            "NumberSequence",
            s.keypoints
                .iter()
                .map(|k| nums(&[k.time, k.value, k.envelope]))
                .collect::<Option<Vec<_>>>()
                .map(Value::Array)?,
        ),
        Variant::ColorSequence(s) => (
            "ColorSequence",
            s.keypoints
                .iter()
                .map(|k| nums(&[k.time, k.color.r, k.color.g, k.color.b, k.envelope]))
                .collect::<Option<Vec<_>>>()
                .map(Value::Array)?,
        ),
        Variant::NumberRange(r) => ("NumberRange", nums(&[r.min, r.max])?),
        Variant::Rect(r) => ("Rect", nums(&[r.min.x, r.min.y, r.max.x, r.max.y])?),
        Variant::PhysicalProperties(p) => (
            "PhysicalProperties",
            match p {
                PhysicalProperties::Default => json!("Default"),
                PhysicalProperties::Custom {
                    density,
                    friction,
                    elasticity,
                    friction_weight,
                    elasticity_weight,
                } => nums(&[
                    *density,
                    *friction,
                    *elasticity,
                    *friction_weight,
                    *elasticity_weight,
                ])?,
            },
        ),
        Variant::UDim(u) => ("UDim", udim_nums(u)?),
        Variant::UDim2(u) => ("UDim2", udim2_nums(u)?),
        Variant::Font(f) => (
            "Font",
            json!({"family": f.family, "weight": f.weight, "style": u8::from(f.style)}),
        ),
        Variant::SecurityCapabilities(n) => ("SecurityCapabilities", json!(n)),
        Variant::Content(Content::Uri(s)) => ("Content", json!(s)),
        Variant::Content(_)
        | Variant::SharedString(_)
        | Variant::UniqueId(_)
        | Variant::Unknown { .. } => return None,
    };
    Some(json!({"t": t, "v": v}))
}

fn udim_nums(u: &UDim) -> Option<Value> {
    Some(Value::Array(vec![
        num(u.scale)?,
        Value::from(i64::from(u.offset)),
    ]))
}

fn udim2_nums(u: &UDim2) -> Option<Value> {
    Some(Value::Array(vec![
        num(u.x.scale)?,
        Value::from(i64::from(u.x.offset)),
        num(u.y.scale)?,
        Value::from(i64::from(u.y.offset)),
    ]))
}

fn floats(v: &Value, n: usize) -> Option<Vec<f32>> {
    let a = v.as_array()?;
    if a.len() != n {
        return None;
    }
    a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect()
}

fn bools(v: &Value, n: usize) -> Option<Vec<bool>> {
    let a = v.as_array()?;
    if a.len() != n {
        return None;
    }
    a.iter().map(Value::as_bool).collect()
}

fn ints(v: &Value, n: usize) -> Option<Vec<i64>> {
    let a = v.as_array()?;
    if a.len() != n {
        return None;
    }
    a.iter().map(Value::as_i64).collect()
}

fn cframe_from(f: &[f32]) -> CFrameData {
    let mut rotation = [0.0; 9];
    rotation.copy_from_slice(&f[3..12]);
    CFrameData {
        position: Vector3Data {
            x: f[0],
            y: f[1],
            z: f[2],
        },
        rotation,
    }
}

fn v3(f: &[f32]) -> Vector3Data {
    Vector3Data {
        x: f[0],
        y: f[1],
        z: f[2],
    }
}

fn rows(v: &Value, width: usize) -> Option<Vec<Vec<f32>>> {
    v.as_array()?.iter().map(|r| floats(r, width)).collect()
}

fn udim_from(scale: &Value, offset: &Value) -> Option<UDim> {
    Some(UDim {
        scale: scale.as_f64()? as f32,
        offset: i32::try_from(offset.as_i64()?).ok()?,
    })
}

/// `None` for an unknown type, a malformed value, or a `Ref` that is `null`
/// or does not resolve (the caller then removes the property, which is how a
/// place stores an empty reference). `resolve` finds a `Ref`'s referent.
pub fn decode(value: &Value, resolve: &dyn Fn(&Target) -> Option<Ref>) -> Option<Variant> {
    let t = value.get("t")?.as_str()?;
    let v = value.get("v")?;
    Some(match t {
        "String" => Variant::String(v.as_str()?.to_string()),
        "Bool" => Variant::Bool(v.as_bool()?),
        "Int32" => Variant::Int32(i32::try_from(v.as_i64()?).ok()?),
        "Int64" => Variant::Int64(v.as_i64()?),
        "Float32" => Variant::Float32(v.as_f64()? as f32),
        "Float64" => Variant::Float64(v.as_f64()?),
        "BrickColor" => Variant::BrickColor(u32::try_from(v.as_u64()?).ok()?),
        "Color3" => {
            let f = floats(v, 3)?;
            Variant::Color3(Color3Data {
                r: f[0],
                g: f[1],
                b: f[2],
            })
        }
        "Color3uint8" => {
            let i = ints(v, 3)?;
            Variant::Color3uint8 {
                r: u8::try_from(i[0]).ok()?,
                g: u8::try_from(i[1]).ok()?,
                b: u8::try_from(i[2]).ok()?,
            }
        }
        "Vector2" => {
            let f = floats(v, 2)?;
            Variant::Vector2(Vector2Data { x: f[0], y: f[1] })
        }
        "Vector3" => Variant::Vector3(v3(&floats(v, 3)?)),
        "Vector3int16" => {
            let i = ints(v, 3)?;
            Variant::Vector3int16 {
                x: i16::try_from(i[0]).ok()?,
                y: i16::try_from(i[1]).ok()?,
                z: i16::try_from(i[2]).ok()?,
            }
        }
        "Ray" => {
            let f = floats(v, 6)?;
            Variant::Ray {
                origin: v3(&f[..3]),
                direction: v3(&f[3..]),
            }
        }
        "Faces" => {
            let b = bools(v, 6)?;
            Variant::Faces(Faces {
                front: b[0],
                bottom: b[1],
                left: b[2],
                back: b[3],
                top: b[4],
                right: b[5],
            })
        }
        "Axes" => {
            let b = bools(v, 3)?;
            Variant::Axes(Axes {
                x: b[0],
                y: b[1],
                z: b[2],
            })
        }
        "CFrame" => Variant::CFrame(cframe_from(&floats(v, 12)?)),
        "OptionalCFrame" => Variant::OptionalCFrame(match v {
            Value::Null => None,
            _ => Some(cframe_from(&floats(v, 12)?)),
        }),
        "Enum" => Variant::Enum(u32::try_from(v.as_u64()?).ok()?),
        "Ref" => {
            if v.is_null() {
                return None;
            }
            let target: Target = serde_json::from_value(v.clone()).ok()?;
            Variant::Ref(resolve(&target)?)
        }
        "NumberSequence" => Variant::NumberSequence(NumberSequence {
            keypoints: rows(v, 3)?
                .iter()
                .map(|r| NumberSequenceKeypoint {
                    time: r[0],
                    value: r[1],
                    envelope: r[2],
                })
                .collect(),
        }),
        "ColorSequence" => Variant::ColorSequence(ColorSequence {
            keypoints: rows(v, 5)?
                .iter()
                .map(|r| ColorSequenceKeypoint {
                    time: r[0],
                    color: Color3Data {
                        r: r[1],
                        g: r[2],
                        b: r[3],
                    },
                    envelope: r[4],
                })
                .collect(),
        }),
        "NumberRange" => {
            let f = floats(v, 2)?;
            Variant::NumberRange(NumberRange {
                min: f[0],
                max: f[1],
            })
        }
        "Rect" => {
            let f = floats(v, 4)?;
            Variant::Rect(Rect {
                min: Vector2Data { x: f[0], y: f[1] },
                max: Vector2Data { x: f[2], y: f[3] },
            })
        }
        "PhysicalProperties" => Variant::PhysicalProperties(match v.as_str() {
            Some("Default") => PhysicalProperties::Default,
            Some(_) => return None,
            None => {
                let f = floats(v, 5)?;
                PhysicalProperties::Custom {
                    density: f[0],
                    friction: f[1],
                    elasticity: f[2],
                    friction_weight: f[3],
                    elasticity_weight: f[4],
                }
            }
        }),
        "UDim" => {
            let a = v.as_array().filter(|a| a.len() == 2)?;
            Variant::UDim(udim_from(&a[0], &a[1])?)
        }
        "UDim2" => {
            let a = v.as_array().filter(|a| a.len() == 4)?;
            Variant::UDim2(UDim2 {
                x: udim_from(&a[0], &a[1])?,
                y: udim_from(&a[2], &a[3])?,
            })
        }
        "Font" => Variant::Font(Font {
            family: v.get("family")?.as_str()?.to_string(),
            weight: u16::try_from(v.get("weight")?.as_u64()?).ok()?,
            style: u8::try_from(v.get("style")?.as_u64()?).ok()?.into(),
            cached_face_id: None,
        }),
        "SecurityCapabilities" => Variant::SecurityCapabilities(v.as_u64()?),
        "Content" => Variant::Content(Content::Uri(v.as_str()?.to_string())),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
