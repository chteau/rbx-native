//! `FloatCurveKey`, `RotationCurveKey` and `ValueCurveKey`: a time, a value
//! (a number, a CFrame, any value), how the curve travels to the next key,
//! and the cubic tangents (nil until set). From the AnimationApi golden: the
//! interpolation defaults to Cubic, a right tangent is refused on a key
//! that isn't Cubic, and `tostring` gives `{Linear, 1, 2, nil, nil}`.

use mlua::{Function, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods};
use mlua::{UserDataRef, Value};

use super::tween_info::enum_item;
use crate::lua_enum::LuaEnumItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyKind {
    Float,
    Rotation,
    Value,
}

impl KeyKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            KeyKind::Float => "FloatCurveKey",
            KeyKind::Rotation => "RotationCurveKey",
            KeyKind::Value => "ValueCurveKey",
        }
    }
}

/// `Enum.KeyInterpolationMode` by value: Constant 0, Linear 1, Cubic 2.
pub(crate) const CUBIC: u32 = 2;
const MODES: [&str; 3] = ["Constant", "Linear", "Cubic"];

#[derive(Clone, Debug)]
pub(crate) struct LuaCurveKey {
    pub(crate) kind: KeyKind,
    pub(crate) time: f32,
    pub(crate) value: Value,
    pub(crate) interpolation: u32,
    pub(crate) left: Option<f32>,
    pub(crate) right: Option<f32>,
}

impl LuaCurveKey {
    pub(crate) fn number(&self) -> f32 {
        match self.value {
            Value::Number(n) => n as f32,
            Value::Integer(i) => i as f32,
            _ => 0.0,
        }
    }
}

/// How an argument converts to a number, as the engine casts it: a
/// non-numeric string is 0.
fn cast_number(value: &Value) -> f32 {
    match value {
        Value::Number(n) => *n as f32,
        Value::Integer(i) => *i as f32,
        Value::String(s) => s.to_str().ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0),
        _ => 0.0,
    }
}

fn mode_arg(value: Option<Value>) -> Result<u32> {
    match value {
        None | Some(Value::Nil) => Ok(CUBIC),
        Some(Value::UserData(data)) => match data.borrow::<LuaEnumItem>() {
            Ok(item) if item.enum_name() == "KeyInterpolationMode" => Ok(item.value()),
            _ => Err(mlua::Error::runtime(
                "Unable to cast value to Enum.KeyInterpolationMode",
            )),
        },
        Some(Value::Number(n)) => Ok(n as u32),
        Some(Value::Integer(i)) => Ok(i as u32),
        Some(other) => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to KeyInterpolationMode",
            other.type_name()
        ))),
    }
}

/// One of the three key datatypes, `K` naming it for `typeof`.
#[derive(Clone, Debug)]
pub(crate) struct Typed<const K: u8>(pub(crate) LuaCurveKey);

pub(crate) type FloatKey = Typed<0>;
pub(crate) type RotationKey = Typed<1>;
pub(crate) type ValueKey = Typed<2>;

/// The key inside `value`, of whichever of the three it is.
pub(crate) fn key_of(value: &Value) -> Option<LuaCurveKey> {
    let Value::UserData(data) = value else {
        return None;
    };
    data.borrow::<FloatKey>()
        .map(|k| k.0.clone())
        .or_else(|_| data.borrow::<RotationKey>().map(|k| k.0.clone()))
        .or_else(|_| data.borrow::<ValueKey>().map(|k| k.0.clone()))
        .ok()
}

/// `key` as the datatype its kind says.
pub(crate) fn into_lua(lua: &Lua, key: LuaCurveKey) -> Result<Value> {
    Ok(Value::UserData(match key.kind {
        KeyKind::Float => lua.create_userdata(Typed::<0>(key))?,
        KeyKind::Rotation => lua.create_userdata(Typed::<1>(key))?,
        KeyKind::Value => lua.create_userdata(Typed::<2>(key))?,
    }))
}

impl<const K: u8> UserData for Typed<K> {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(
            MetaMethod::Type,
            [KeyKind::Float, KeyKind::Rotation, KeyKind::Value][K as usize].name(),
        );
        fields.add_field_method_get("Time", |_, Typed(this)| Ok(this.time));
        fields.add_field_method_get("Value", |_, Typed(this)| Ok(this.value.clone()));
        fields.add_field_method_get("Interpolation", |lua, Typed(this)| {
            enum_item(lua, "KeyInterpolationMode", MODES[this.interpolation.min(2) as usize])
        });
        fields.add_field_method_get("LeftTangent", |_, Typed(this)| Ok(this.left));
        fields.add_field_method_get("RightTangent", |_, Typed(this)| Ok(this.right));
        fields.add_field_method_set("LeftTangent", |_, Typed(this), value: Option<f32>| {
            this.left = value;
            Ok(())
        });
        fields.add_field_method_set("RightTangent", |_, Typed(this), value: Option<f32>| {
            if value.is_some() && this.interpolation != CUBIC {
                return Err(mlua::Error::runtime(
                    "Attempting to set a right tangent value on a curve key for which the interpolation mode is not Cubic",
                ));
            }
            this.right = value;
            Ok(())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, Typed(this), other: UserDataRef<Self>| {
            let other = &other.0;
            Ok(this.kind == other.kind
                && this.time == other.time
                && this.interpolation == other.interpolation
                && this.left == other.left
                && this.right == other.right
                && this.value.equals(&other.value)?)
        });
        methods.add_meta_method(MetaMethod::ToString, |lua, Typed(this), ()| {
            let tostring: Function = lua.globals().get("tostring")?;
            let text = |v: Value| -> Result<String> { tostring.call::<String>(v) };
            Ok(format!(
                "{{{}, {}, {}, {}, {}}}",
                MODES[this.interpolation.min(2) as usize],
                text(Value::Number(f64::from(this.time)))?,
                text(this.value.clone())?,
                text(this.left.map_or(Value::Nil, |l| Value::Number(f64::from(l))))?,
                text(this.right.map_or(Value::Nil, |r| Value::Number(f64::from(r))))?,
            ))
        });
    }
}

fn constructor(lua: &Lua, kind: KeyKind) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(move |lua, (time, value, mode): (Value, Value, Option<Value>)| {
            let value = match kind {
                KeyKind::Float => Value::Number(f64::from(cast_number(&value))),
                _ => value,
            };
            let key = LuaCurveKey {
                kind,
                time: cast_number(&time),
                value,
                interpolation: mode_arg(mode)?,
                left: None,
                right: None,
            };
            into_lua(lua, key)
        })?,
    )?;
    Ok(table)
}

pub(super) fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    globals.set("FloatCurveKey", constructor(lua, KeyKind::Float)?)?;
    globals.set("RotationCurveKey", constructor(lua, KeyKind::Rotation)?)?;
    globals.set("ValueCurveKey", constructor(lua, KeyKind::Value)?)?;
    Ok(())
}
