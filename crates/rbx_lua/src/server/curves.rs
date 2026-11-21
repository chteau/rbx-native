//! The curve instances' methods: `FloatCurve`, `RotationCurve` and
//! `MarkerCurve` keep their keys in the `ValuesAndTimes` bytes a saved
//! curve has (so a copy keeps them and the Animator plays them);
//! `Vector3Curve` and `EulerRotationCurve` read their X, Y and Z
//! FloatCurves; `ValueCurve` answers empty, as a live server's does.
//! Behaviour from the AnimationApi golden: `InsertKey` answers
//! `{inserted, index from 0}`, the other indices count from 1, `Length` is
//! the key count, an unset tangent is a flat one.

use mlua::{IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_animation::curve::values::{self, RawMarker, RawRotationKey, TICKS_PER_SECOND};
use rbx_animation::curve::{FloatCurve, FloatKey, Interpolation, RotationOrder};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::datatypes::cframe::ops::{lerp, matrix_to_quat, quat_to_matrix};
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::extra::curve_key::{self, KeyKind, LuaCurveKey, CUBIC};
use crate::instance::LuaInstance;

const CLASSES: &[&str] = &[
    "FloatCurve",
    "RotationCurve",
    "MarkerCurve",
    "ValueCurve",
    "Vector3Curve",
    "EulerRotationCurve",
];

/// One key whatever the curve: a number, a rotation (x, y, z, w) or a marker.
#[derive(Clone, Debug)]
enum Stored {
    Float(f32),
    Rotation([f32; 4]),
    Marker(String),
}

#[derive(Clone, Debug)]
struct Key {
    ticks: u32,
    value: Stored,
    interpolation: u32,
    left: Option<f32>,
    right: Option<f32>,
}

impl Key {
    fn time(&self) -> f32 {
        values::seconds(self.ticks)
    }

    fn tangents(&self) -> (f32, f32) {
        let left = self.left.or(self.right).unwrap_or(0.0);
        let right = self.right.or(self.left).unwrap_or(0.0);
        (left, right)
    }
}

fn ticks(time: f32) -> u32 {
    (time * TICKS_PER_SECOND).round().max(0.0) as u32
}

fn raw(ctx: &Ctx, curve: Ref) -> Vec<u8> {
    let dom = ctx.dom();
    match dom
        .get(curve)
        .and_then(|i| i.properties().get("ValuesAndTimes"))
    {
        Some(Variant::String(s)) => s.as_bytes().to_vec(),
        Some(Variant::Unknown { raw, .. }) => raw.clone(),
        _ => Vec::new(),
    }
}

fn read(ctx: &Ctx, curve: Ref, class: &str) -> Vec<Key> {
    let bytes = raw(ctx, curve);
    if bytes.is_empty() {
        return Vec::new();
    }
    match class {
        "FloatCurve" => values::read_float(&bytes)
            .map(|keys| {
                values::to_curve(&keys)
                    .keys
                    .into_iter()
                    .map(|k| Key {
                        ticks: ticks(k.time),
                        value: Stored::Float(k.value),
                        interpolation: k.interpolation.value(),
                        left: k.left_tangent,
                        right: k.right_tangent,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "RotationCurve" => values::read_rotation(&bytes)
            .map(|keys| {
                keys.into_iter()
                    .map(|k| Key {
                        ticks: k.ticks,
                        value: Stored::Rotation(k.rotation),
                        interpolation: u32::from(k.interpolation),
                        left: (k.left != 0.0).then_some(k.left),
                        right: (k.right != 0.0).then_some(k.right),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "MarkerCurve" => values::read_markers(&bytes)
            .map(|markers| {
                markers
                    .into_iter()
                    .map(|m| Key {
                        ticks: m.ticks,
                        value: Stored::Marker(String::from_utf8_lossy(&m.value).into_owned()),
                        interpolation: 0,
                        left: None,
                        right: None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn write(ctx: &Ctx, curve: Ref, class: &str, keys: &[Key]) {
    let raw = match class {
        "FloatCurve" => {
            let curve = FloatCurve {
                keys: keys
                    .iter()
                    .map(|k| FloatKey {
                        time: k.time(),
                        value: match k.value {
                            Stored::Float(v) => v,
                            _ => 0.0,
                        },
                        interpolation: Interpolation::from_value(k.interpolation),
                        left_tangent: k.left,
                        right_tangent: k.right,
                    })
                    .collect(),
            };
            values::write_float(&values::from_curve(&curve))
        }
        "RotationCurve" => values::write_rotation(
            &keys
                .iter()
                .map(|k| RawRotationKey {
                    ticks: k.ticks,
                    interpolation: k.interpolation as u8,
                    rotation: match k.value {
                        Stored::Rotation(q) => q,
                        _ => [0.0, 0.0, 0.0, 1.0],
                    },
                    left: k.left.unwrap_or(0.0),
                    right: k.right.unwrap_or(0.0),
                })
                .collect::<Vec<_>>(),
        ),
        _ => values::write_markers(
            &keys
                .iter()
                .map(|k| RawMarker {
                    ticks: k.ticks,
                    value: match &k.value {
                        Stored::Marker(m) => m.as_bytes().to_vec(),
                        _ => Vec::new(),
                    },
                })
                .collect::<Vec<_>>(),
        ),
    };
    let _ = ctx.dom_mut().set_property(
        curve,
        "ValuesAndTimes",
        Variant::Unknown { type_id: 0x01, raw },
    );
}

/// Puts `key` in time order, replacing one at the same tick: `(inserted,
/// index from 0)`.
fn insert(keys: &mut Vec<Key>, key: Key) -> (bool, usize) {
    match keys.binary_search_by_key(&key.ticks, |k| k.ticks) {
        Ok(i) => {
            keys[i] = key;
            (false, i)
        }
        Err(i) => {
            keys.insert(i, key);
            (true, i)
        }
    }
}

/// The 1-based indices of the segment `time` falls in, the first or last
/// one outside the keys.
fn indices(keys: &[Key], time: f32) -> Vec<usize> {
    match keys.len() {
        0 => Vec::new(),
        1 => vec![1],
        n => {
            let at = keys.partition_point(|k| k.time() <= time);
            let i = at.clamp(1, n - 1);
            vec![i, i + 1]
        }
    }
}

/// The segment around `time` and how far along it: `None` past either end,
/// where the curve holds that key.
fn segment(keys: &[Key], time: f32) -> std::result::Result<(usize, Option<f32>), usize> {
    let at = keys.partition_point(|k| k.time() <= time);
    if at == 0 {
        return Err(0);
    }
    if at == keys.len() {
        return Err(at - 1);
    }
    let (k0, k1) = (&keys[at - 1], &keys[at]);
    let h = k1.time() - k0.time();
    Ok((at - 1, (h > 0.0).then(|| (time - k0.time()) / h)))
}

fn hermite(v0: f32, v1: f32, m0: f32, m1: f32, h: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (2. * u3 - 3. * u2 + 1.) * v0
        + (u3 - 2. * u2 + u) * h * m0
        + (-2. * u3 + 3. * u2) * v1
        + (u3 - u2) * h * m1
}

fn float_at(keys: &[Key], time: f32) -> Option<f32> {
    let value = |k: &Key| match k.value {
        Stored::Float(v) => v,
        _ => 0.0,
    };
    if keys.is_empty() {
        return None;
    }
    Some(match segment(keys, time) {
        Err(i) => value(&keys[i]),
        Ok((i, u)) => {
            let (k0, k1) = (&keys[i], &keys[i + 1]);
            let u = u.unwrap_or(1.0);
            match k0.interpolation {
                0 => value(k0),
                CUBIC => hermite(
                    value(k0),
                    value(k1),
                    k0.tangents().1,
                    k1.tangents().0,
                    k1.time() - k0.time(),
                    u,
                ),
                _ => value(k0) + (value(k1) - value(k0)) * u,
            }
        }
    })
}

fn rotation_cframe(q: [f32; 4]) -> LuaCFrame {
    quat_to_matrix(q, rbx_dom::Vector3Data { x: 0.0, y: 0.0, z: 0.0 })
}

fn rotation_at(keys: &[Key], time: f32) -> Option<LuaCFrame> {
    let q = |k: &Key| match k.value {
        Stored::Rotation(q) => q,
        _ => [0.0, 0.0, 0.0, 1.0],
    };
    if keys.is_empty() {
        return None;
    }
    Some(match segment(keys, time) {
        Err(i) => rotation_cframe(q(&keys[i])),
        Ok((i, u)) => {
            let (k0, k1) = (&keys[i], &keys[i + 1]);
            let u = u.unwrap_or(1.0);
            let s = match k0.interpolation {
                0 => 0.0,
                CUBIC => {
                    let h = k1.time() - k0.time();
                    let (u2, u3) = (u * u, u * u * u);
                    (3. * u2 - 2. * u3)
                        + (u3 - 2. * u2 + u) * k0.tangents().1 * h
                        + (u3 - u2) * k1.tangents().0 * h
                }
                _ => u,
            };
            lerp(&rotation_cframe(q(k0)), &rotation_cframe(q(k1)), s)
        }
    })
}

/// The engine's name for a value's type in a failed cast.
fn cast_name(value: &Value) -> String {
    match value {
        Value::Nil => "nil".into(),
        Value::Boolean(_) => "bool".into(),
        Value::Integer(_) | Value::Number(_) => "double".into(),
        Value::String(_) => "string".into(),
        Value::Table(_) => "table".into(),
        Value::UserData(u) => u
            .metatable()
            .and_then(|m| m.get::<String>("__type"))
            .unwrap_or_else(|_| "userdata".into()),
        other => other.type_name().into(),
    }
}

fn key_arg(value: &Value, kind: KeyKind) -> Result<LuaCurveKey> {
    match curve_key::key_of(value) {
        Some(key) if key.kind == kind => Ok(key),
        _ => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to {}",
            cast_name(value),
            kind.name()
        ))),
    }
}

fn kind_of(class: &str) -> KeyKind {
    match class {
        "RotationCurve" => KeyKind::Rotation,
        "ValueCurve" => KeyKind::Value,
        _ => KeyKind::Float,
    }
}

fn stored(key: &LuaCurveKey) -> Result<Key> {
    let value = match key.kind {
        KeyKind::Rotation => match &key.value {
            Value::UserData(u) => Stored::Rotation(matrix_to_quat(&*u.borrow::<LuaCFrame>()?)),
            _ => Stored::Rotation([0.0, 0.0, 0.0, 1.0]),
        },
        _ => Stored::Float(key.number()),
    };
    Ok(Key {
        ticks: ticks(key.time),
        value,
        interpolation: key.interpolation,
        left: key.left,
        right: key.right,
    })
}

fn key_value(lua: &Lua, class: &str, key: &Key) -> Result<Value> {
    let value = match &key.value {
        Stored::Float(v) => Value::Number(f64::from(*v)),
        Stored::Rotation(q) => rotation_cframe(*q).into_lua(lua)?,
        Stored::Marker(m) => m.as_str().into_lua(lua)?,
    };
    curve_key::into_lua(
        lua,
        LuaCurveKey {
            kind: kind_of(class),
            time: key.time(),
            value,
            interpolation: key.interpolation,
            left: key.left,
            right: key.right,
        },
    )
}

fn marker(lua: &Lua, key: &Key) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("Time", key.time())?;
    if let Stored::Marker(m) = &key.value {
        t.set("Value", m.as_str())?;
    }
    Ok(t)
}

fn index_arg(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Integer(i)) => *i,
        Some(Value::Number(n)) => *n as i64,
        _ => 0,
    }
}

/// The child FloatCurve `name` (X, Y or Z), made when missing.
fn axis(ctx: &Ctx, curve: Ref, name: &str) -> Ref {
    let found = {
        let dom = ctx.dom();
        dom.get(curve).and_then(|i| {
            i.children().iter().copied().find(|c| {
                dom.get(*c)
                    .is_some_and(|c| c.name() == name && c.class() == "FloatCurve")
            })
        })
    };
    found.unwrap_or_else(|| ctx.dom_mut().new_instance("FloatCurve", name, Some(curve)))
}

/// The X, Y and Z values at `time`, nil where a channel has no keys.
fn axes_at(ctx: &Ctx, curve: Ref, time: f32) -> [Option<f32>; 3] {
    ["X", "Y", "Z"].map(|name| {
        let channel = {
            let dom = ctx.dom();
            dom.get(curve).and_then(|i| {
                i.children().iter().copied().find(|c| {
                    dom.get(*c)
                        .is_some_and(|c| c.name() == name && c.class() == "FloatCurve")
                })
            })
        };
        channel.and_then(|c| float_at(&read(ctx, c, "FloatCurve"), time))
    })
}

fn number_list(lua: &Lua, values: &[Option<f32>]) -> Result<Table> {
    let t = lua.create_table()?;
    for (i, v) in values.iter().enumerate() {
        if let Some(v) = v {
            t.raw_set(i + 1, *v)?;
        }
    }
    Ok(t)
}

fn pair(lua: &Lua, inserted: bool, index: usize) -> Result<Table> {
    lua.create_sequence_from([Value::Boolean(inserted), Value::Integer(index as i64)])
}

/// `curve[key]` for the curve classes, `None` for anything else.
pub(crate) fn member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    if !CLASSES.contains(&class) {
        return Ok(None);
    }
    let class = class.to_owned();
    if key == "Length" && class != "ValueCurve" {
        if matches!(class.as_str(), "Vector3Curve" | "EulerRotationCurve") {
            return Ok(None);
        }
        return Ok(Some(Value::Integer(read(ctx, referent, &class).len() as i64)));
    }
    if key == "ValueType" && class == "ValueCurve" {
        return Ok(Some(Value::Nil));
    }
    let ctx = ctx.clone();
    let method = |f: Box<dyn Fn(&Lua, &[Value]) -> Result<Value>>| -> Result<Option<Value>> {
        let f = lua.create_function(move |lua, args: MultiValue| {
            let args: Vec<Value> = args.into_iter().skip(1).collect();
            f(lua, &args)
        })?;
        Ok(Some(Value::Function(f)))
    };
    let out_of_range = |class: &str| {
        mlua::Error::runtime(format!(
            "{} index is out of range",
            if class == "MarkerCurve" { "MarkerCurve" } else { class }
        ))
    };
    match (class.as_str(), key) {
        ("ValueCurve", "GetKeys" | "InsertKey" | "InsertKeyValue" | "GetKeyIndicesAtTime") => {
            method(Box::new(|lua, _| lua.create_table().map(Value::Table)))
        }
        ("ValueCurve", "GetValueAtTime") => method(Box::new(|_, _| Ok(Value::Nil))),
        ("ValueCurve", "RemoveKeyAtIndex" | "SetKeys") => {
            method(Box::new(|_, _| Ok(Value::Integer(0))))
        }
        ("ValueCurve", "GetKeyAtIndex") => {
            method(Box::new(|_, _| Err(mlua::Error::runtime("ValueCurve index is out of range"))))
        }
        ("FloatCurve" | "RotationCurve", "GetKeys") => method(Box::new(move |lua, _| {
            let keys = read(&ctx, referent, &class);
            let list = lua.create_table()?;
            for k in &keys {
                list.push(key_value(lua, &class, k)?)?;
            }
            Ok(Value::Table(list))
        })),
        ("FloatCurve" | "RotationCurve", "GetKeyAtIndex") => {
            method(Box::new(move |lua, args| {
                let keys = read(&ctx, referent, &class);
                let i = index_arg(args.first());
                match usize::try_from(i - 1).ok().and_then(|i| keys.get(i)) {
                    Some(k) => key_value(lua, &class, k),
                    None => Err(out_of_range(&class)),
                }
            }))
        }
        ("FloatCurve" | "RotationCurve" | "MarkerCurve", "GetKeyIndicesAtTime") => {
            method(Box::new(move |lua, args| {
                let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
                let keys = read(&ctx, referent, &class);
                lua.create_sequence_from(indices(&keys, time)).map(Value::Table)
            }))
        }
        ("FloatCurve", "GetValueAtTime") => method(Box::new(move |_, args| {
            let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
            let keys = read(&ctx, referent, &class);
            Ok(float_at(&keys, time).map_or(Value::Nil, |v| Value::Number(f64::from(v))))
        })),
        ("RotationCurve", "GetValueAtTime") => method(Box::new(move |lua, args| {
            let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
            let keys = read(&ctx, referent, &class);
            match rotation_at(&keys, time) {
                Some(c) => c.into_lua(lua),
                None => Ok(Value::Nil),
            }
        })),
        ("FloatCurve" | "RotationCurve", "InsertKey") => method(Box::new(move |lua, args| {
            let key = key_arg(args.first().unwrap_or(&Value::Nil), kind_of(&class))?;
            let mut keys = read(&ctx, referent, &class);
            let (inserted, i) = insert(&mut keys, stored(&key)?);
            write(&ctx, referent, &class, &keys);
            pair(lua, inserted, i).map(Value::Table)
        })),
        ("FloatCurve" | "RotationCurve", "SetKeys") => method(Box::new(move |_, args| {
            let list = match args.first() {
                Some(Value::Table(t)) => t.clone(),
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "Unable to cast {} to Array",
                        cast_name(other.unwrap_or(&Value::Nil))
                    )))
                }
            };
            let mut keys = Vec::new();
            for value in list.sequence_values::<Value>() {
                let key = curve_key::key_of(&value?)
                    .filter(|k| k.kind == kind_of(&class))
                    .ok_or_else(|| mlua::Error::runtime("Variant cast failed"))?;
                insert(&mut keys, stored(&key)?);
            }
            write(&ctx, referent, &class, &keys);
            Ok(Value::Integer(keys.len() as i64))
        })),
        ("FloatCurve" | "RotationCurve", "RemoveKeyAtIndex")
        | ("MarkerCurve", "RemoveMarkerAtIndex") => method(Box::new(move |_, args| {
            let start = index_arg(args.first());
            let count = match args.get(1) {
                None | Some(Value::Nil) => 1,
                other => index_arg(other),
            };
            let mut keys = read(&ctx, referent, &class);
            if start < 1 || start as usize > keys.len() || count < 1 {
                return Ok(Value::Integer(0));
            }
            let from = start as usize - 1;
            let to = (from + count as usize).min(keys.len());
            keys.drain(from..to);
            write(&ctx, referent, &class, &keys);
            Ok(Value::Integer((to - from) as i64))
        })),
        ("MarkerCurve", "InsertMarkerAtTime") => method(Box::new(move |lua, args| {
            let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
            let text = match args.get(1) {
                Some(Value::String(s)) => s.to_str()?.to_owned(),
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::Integer(i)) => i.to_string(),
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "Unable to cast {} to string",
                        cast_name(other.unwrap_or(&Value::Nil))
                    )))
                }
            };
            let mut keys = read(&ctx, referent, &class);
            let key = Key {
                ticks: ticks(time),
                value: Stored::Marker(text),
                interpolation: 0,
                left: None,
                right: None,
            };
            let (inserted, i) = insert(&mut keys, key);
            write(&ctx, referent, &class, &keys);
            pair(lua, inserted, i).map(Value::Table)
        })),
        ("MarkerCurve", "GetMarkers") => method(Box::new(move |lua, _| {
            let list = lua.create_table()?;
            for k in &read(&ctx, referent, &class) {
                list.push(marker(lua, k)?)?;
            }
            Ok(Value::Table(list))
        })),
        ("MarkerCurve", "GetMarkerAtIndex") => method(Box::new(move |lua, args| {
            let keys = read(&ctx, referent, &class);
            let i = index_arg(args.first());
            match usize::try_from(i - 1).ok().and_then(|i| keys.get(i)) {
                Some(k) => marker(lua, k).map(Value::Table),
                None => Err(out_of_range(&class)),
            }
        })),
        ("Vector3Curve" | "EulerRotationCurve", "X" | "Y" | "Z") => {
            let name = key.to_owned();
            method(Box::new(move |lua, _| {
                LuaInstance::new(axis(&ctx, referent, &name), ctx.clone()).into_lua(lua)
            }))
        }
        ("Vector3Curve", "GetValueAtTime") | ("EulerRotationCurve", "GetAnglesAtTime") => {
            method(Box::new(move |lua, args| {
                let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
                number_list(lua, &axes_at(&ctx, referent, time)).map(Value::Table)
            }))
        }
        ("EulerRotationCurve", "GetRotationAtTime") => method(Box::new(move |lua, args| {
            let time = args.first().and_then(crate::datatypes::number_arg).unwrap_or(0.0);
            let [x, y, z] = axes_at(&ctx, referent, time).map(|a| a.unwrap_or(0.0));
            let order = match ctx
                .dom()
                .get(referent)
                .and_then(|i| i.properties().get("RotationOrder"))
            {
                Some(Variant::Enum(e)) => *e,
                _ => 0,
            };
            let q = RotationOrder::from_value(order).compose(glam::Vec3::new(x, y, z));
            rotation_cframe(q.to_array()).into_lua(lua)
        })),
        _ => Ok(None),
    }
}
