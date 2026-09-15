//! `NumberSequence`/`ColorSequence` and their keypoints.

use mlua::{
    Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic,
};
use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint,
};

use super::color3::LuaColor3;
use super::{from_userdata, number_arg};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaNumberSequenceKeypoint(pub(crate) NumberSequenceKeypoint);

impl UserData for LuaNumberSequenceKeypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Time", |_, this| Ok(this.0.time));
        fields.add_field_method_get("Value", |_, this| Ok(this.0.value));
        fields.add_field_method_get("Envelope", |_, this| Ok(this.0.envelope));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: LuaNumberSequenceKeypoint| Ok(*this == other),
        );
    }
}

impl mlua::FromLua for LuaNumberSequenceKeypoint {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "NumberSequenceKeypoint")
    }
}

/// Wraps a non-`Copy` `Vec`-backed value; the `Keypoints` array is rebuilt on
/// each read rather than cached, matching how `GetChildren` builds its table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LuaNumberSequence(pub(crate) NumberSequence);

impl UserData for LuaNumberSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Keypoints", |lua, this| {
            let table = lua.create_table()?;
            for (index, keypoint) in this.0.keypoints.iter().enumerate() {
                table.set(index + 1, LuaNumberSequenceKeypoint(*keypoint))?;
            }
            Ok(table)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaNumberSequence| {
            Ok(*this == other)
        });
    }
}

impl mlua::FromLua for LuaNumberSequence {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        if let Value::UserData(data) = &value {
            if let Ok(inner) = data.borrow::<Self>() {
                return Ok(inner.clone());
            }
        }
        Err(mlua::Error::runtime(format!(
            "NumberSequence expected, got {}",
            value.type_name()
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaColorSequenceKeypoint(pub(crate) ColorSequenceKeypoint);

impl UserData for LuaColorSequenceKeypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Time", |_, this| Ok(this.0.time));
        fields.add_field_method_get("Value", |_, this| Ok(LuaColor3(this.0.color)));
        fields.add_field_method_get("Envelope", |_, this| Ok(this.0.envelope));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: LuaColorSequenceKeypoint| Ok(*this == other),
        );
    }
}

impl mlua::FromLua for LuaColorSequenceKeypoint {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "ColorSequenceKeypoint")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LuaColorSequence(pub(crate) ColorSequence);

impl UserData for LuaColorSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Keypoints", |lua, this| {
            let table = lua.create_table()?;
            for (index, keypoint) in this.0.keypoints.iter().enumerate() {
                table.set(index + 1, LuaColorSequenceKeypoint(*keypoint))?;
            }
            Ok(table)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaColorSequence| {
            Ok(*this == other)
        });
    }
}

impl mlua::FromLua for LuaColorSequence {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        if let Value::UserData(data) = &value {
            if let Ok(inner) = data.borrow::<Self>() {
                return Ok(inner.clone());
            }
        }
        Err(mlua::Error::runtime(format!(
            "ColorSequence expected, got {}",
            value.type_name()
        )))
    }
}

fn color_arg(value: &Value) -> Option<Color3Data> {
    if let Value::UserData(data) = value {
        if let Ok(color) = data.borrow::<LuaColor3>() {
            return Some(color.0);
        }
    }
    None
}

pub(crate) fn number_sequence_keypoint(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (time, value, envelope): (f32, f32, Option<f32>)| {
            Ok(LuaNumberSequenceKeypoint(NumberSequenceKeypoint {
                time,
                value,
                envelope: envelope.unwrap_or_default(),
            }))
        })?,
    )?;
    Ok(table)
}

pub(crate) fn color_sequence_keypoint(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (time, color, envelope): (f32, LuaColor3, Option<f32>)| {
                Ok(LuaColorSequenceKeypoint(ColorSequenceKeypoint {
                    time,
                    color: color.0,
                    envelope: envelope.unwrap_or_default(),
                }))
            },
        )?,
    )?;
    Ok(table)
}

const NUMBER_SEQUENCE_USAGE: &str =
    "NumberSequence.new expects a number, two numbers, or a table of keypoints";

pub(crate) fn number_sequence(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, args: Variadic<Value>| match args.as_slice() {
            [Value::Table(points)] => {
                let mut keypoints = Vec::new();
                for keypoint in points.sequence_values::<LuaNumberSequenceKeypoint>() {
                    keypoints.push(keypoint?.0);
                }
                if keypoints.is_empty() {
                    return Err(mlua::Error::runtime(NUMBER_SEQUENCE_USAGE));
                }
                Ok(LuaNumberSequence(NumberSequence { keypoints }))
            }
            [single] => {
                let value = number_arg(single)
                    .ok_or_else(|| mlua::Error::runtime(NUMBER_SEQUENCE_USAGE))?;
                Ok(LuaNumberSequence(NumberSequence {
                    keypoints: vec![
                        NumberSequenceKeypoint {
                            time: 0.0,
                            value,
                            envelope: 0.0,
                        },
                        NumberSequenceKeypoint {
                            time: 1.0,
                            value,
                            envelope: 0.0,
                        },
                    ],
                }))
            }
            [a, b] => {
                let a = number_arg(a).ok_or_else(|| mlua::Error::runtime(NUMBER_SEQUENCE_USAGE))?;
                let b = number_arg(b).ok_or_else(|| mlua::Error::runtime(NUMBER_SEQUENCE_USAGE))?;
                Ok(LuaNumberSequence(NumberSequence {
                    keypoints: vec![
                        NumberSequenceKeypoint {
                            time: 0.0,
                            value: a,
                            envelope: 0.0,
                        },
                        NumberSequenceKeypoint {
                            time: 1.0,
                            value: b,
                            envelope: 0.0,
                        },
                    ],
                }))
            }
            _ => Err(mlua::Error::runtime(NUMBER_SEQUENCE_USAGE)),
        })?,
    )?;
    Ok(table)
}

const COLOR_SEQUENCE_USAGE: &str =
    "ColorSequence.new expects a Color3, two Color3s, or a table of keypoints";

pub(crate) fn color_sequence(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, args: Variadic<Value>| match args.as_slice() {
            [Value::Table(points)] => {
                let mut keypoints = Vec::new();
                for keypoint in points.sequence_values::<LuaColorSequenceKeypoint>() {
                    keypoints.push(keypoint?.0);
                }
                if keypoints.is_empty() {
                    return Err(mlua::Error::runtime(COLOR_SEQUENCE_USAGE));
                }
                Ok(LuaColorSequence(ColorSequence { keypoints }))
            }
            [single] => {
                let color =
                    color_arg(single).ok_or_else(|| mlua::Error::runtime(COLOR_SEQUENCE_USAGE))?;
                Ok(LuaColorSequence(ColorSequence {
                    keypoints: vec![
                        ColorSequenceKeypoint {
                            time: 0.0,
                            color,
                            envelope: 0.0,
                        },
                        ColorSequenceKeypoint {
                            time: 1.0,
                            color,
                            envelope: 0.0,
                        },
                    ],
                }))
            }
            [a, b] => {
                let a = color_arg(a).ok_or_else(|| mlua::Error::runtime(COLOR_SEQUENCE_USAGE))?;
                let b = color_arg(b).ok_or_else(|| mlua::Error::runtime(COLOR_SEQUENCE_USAGE))?;
                Ok(LuaColorSequence(ColorSequence {
                    keypoints: vec![
                        ColorSequenceKeypoint {
                            time: 0.0,
                            color: a,
                            envelope: 0.0,
                        },
                        ColorSequenceKeypoint {
                            time: 1.0,
                            color: b,
                            envelope: 0.0,
                        },
                    ],
                }))
            }
            _ => Err(mlua::Error::runtime(COLOR_SEQUENCE_USAGE)),
        })?,
    )?;
    Ok(table)
}
