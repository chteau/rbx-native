//! Smaller datatypes: `Vector2`, `UDim`, `UDim2`, `NumberRange`, `BrickColor`.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{NumberRange, UDim, UDim2, Vector2Data};

use super::{from_userdata, number_arg};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaVector2(pub(crate) Vector2Data);

impl LuaVector2 {
    fn new(x: f32, y: f32) -> Self {
        LuaVector2(Vector2Data { x, y })
    }
}

fn combine2(lhs: &LuaVector2, rhs: &Value, op: fn(f32, f32) -> f32) -> Result<LuaVector2> {
    if let Value::UserData(data) = rhs {
        if let Ok(other) = data.borrow::<LuaVector2>() {
            return Ok(LuaVector2::new(
                op(lhs.0.x, other.0.x),
                op(lhs.0.y, other.0.y),
            ));
        }
    }
    let scalar = number_arg(rhs)
        .ok_or_else(|| mlua::Error::runtime("Vector2 expected a Vector2 or a number"))?;
    Ok(LuaVector2::new(op(lhs.0.x, scalar), op(lhs.0.y, scalar)))
}

impl UserData for LuaVector2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("X", |_, this| Ok(this.0.x));
        fields.add_field_method_get("Y", |_, this| Ok(this.0.y));
        fields.add_field_method_get("Magnitude", |_, this| {
            Ok((this.0.x * this.0.x + this.0.y * this.0.y).sqrt())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Add, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a + b)
        });
        methods.add_meta_method(MetaMethod::Sub, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a - b)
        });
        methods.add_meta_method(MetaMethod::Mul, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a * b)
        });
        methods.add_meta_method(MetaMethod::Div, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a / b)
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaVector2| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}", this.0.x, this.0.y))
        });
    }
}

impl mlua::FromLua for LuaVector2 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Vector2")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaUDim(pub(crate) UDim);

impl UserData for LuaUDim {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Scale", |_, this| Ok(this.0.scale));
        fields.add_field_method_get("Offset", |_, this| Ok(this.0.offset));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaUDim| Ok(*this == other));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}", this.0.scale, this.0.offset))
        });
    }
}

impl mlua::FromLua for LuaUDim {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "UDim")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaUDim2(pub(crate) UDim2);

impl UserData for LuaUDim2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("X", |_, this| Ok(LuaUDim(this.0.x)));
        fields.add_field_method_get("Y", |_, this| Ok(LuaUDim(this.0.y)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: LuaUDim2| Ok(*this == other),
        );
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "{}, {}, {}, {}",
                this.0.x.scale, this.0.x.offset, this.0.y.scale, this.0.y.offset
            ))
        });
    }
}

impl mlua::FromLua for LuaUDim2 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "UDim2")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaNumberRange(pub(crate) NumberRange);

impl UserData for LuaNumberRange {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Min", |_, this| Ok(this.0.min));
        fields.add_field_method_get("Max", |_, this| Ok(this.0.max));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaNumberRange| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{} {}", this.0.min, this.0.max))
        });
    }
}

impl mlua::FromLua for LuaNumberRange {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "NumberRange")
    }
}

/// A hand-picked slice of Roblox's ~208-entry BrickColor palette: enough names
/// for common scripts to resolve, not a full reproduction (the API dump carries
/// no name/colour table to derive one from). Values checked against Roblox's
/// published palette (number, then RGB): `Number`s 1, 21, 23, 24, 26, 37, 194,
/// 1003, 1004.
const NAMED: &[(&str, u32, u8, u8, u8)] = &[
    ("White", 1, 242, 243, 243),
    ("Bright red", 21, 196, 40, 28),
    ("Bright blue", 23, 13, 105, 172),
    ("Bright yellow", 24, 245, 205, 48),
    ("Black", 26, 27, 42, 53),
    ("Bright green", 37, 75, 151, 75),
    ("Medium stone grey", 194, 163, 162, 165),
    ("Really black", 1003, 17, 17, 17),
    ("Really red", 1004, 255, 0, 0),
];

/// A palette index into Roblox's fixed BrickColor table, with the RGB triple
/// when `number` is one of the `NAMED` entries above (needed to write a
/// `BrickColor` into a part's `Color3uint8`; unlisted numbers carry `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LuaBrickColor {
    pub(crate) number: u32,
    pub(crate) color: Option<(u8, u8, u8)>,
}

impl LuaBrickColor {
    pub(crate) fn from_number(number: u32) -> Self {
        let color = NAMED
            .iter()
            .find(|(_, candidate, ..)| *candidate == number)
            .map(|(_, _, r, g, b)| (*r, *g, *b));
        LuaBrickColor { number, color }
    }
}

impl UserData for LuaBrickColor {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Number", |_, this| Ok(this.number));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaBrickColor| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.number.to_string())
        });
    }
}

impl mlua::FromLua for LuaBrickColor {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "BrickColor")
    }
}

pub(crate) fn vector2(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| {
            Ok(LuaVector2::new(
                x.unwrap_or_default(),
                y.unwrap_or_default(),
            ))
        })?,
    )?;
    Ok(table)
}

pub(crate) fn udim(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (scale, offset): (Option<f32>, Option<i32>)| {
            Ok(LuaUDim(UDim {
                scale: scale.unwrap_or_default(),
                offset: offset.unwrap_or_default(),
            }))
        })?,
    )?;
    Ok(table)
}

pub(crate) fn udim2(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (xs, xo, ys, yo): (Option<f32>, Option<i32>, Option<f32>, Option<i32>)| {
                Ok(LuaUDim2(UDim2 {
                    x: UDim {
                        scale: xs.unwrap_or_default(),
                        offset: xo.unwrap_or_default(),
                    },
                    y: UDim {
                        scale: ys.unwrap_or_default(),
                        offset: yo.unwrap_or_default(),
                    },
                }))
            },
        )?,
    )?;
    table.set(
        "fromScale",
        lua.create_function(|_, (x, y): (f32, f32)| {
            Ok(LuaUDim2(UDim2 {
                x: UDim {
                    scale: x,
                    offset: 0,
                },
                y: UDim {
                    scale: y,
                    offset: 0,
                },
            }))
        })?,
    )?;
    table.set(
        "fromOffset",
        lua.create_function(|_, (x, y): (i32, i32)| {
            Ok(LuaUDim2(UDim2 {
                x: UDim {
                    scale: 0.0,
                    offset: x,
                },
                y: UDim {
                    scale: 0.0,
                    offset: y,
                },
            }))
        })?,
    )?;
    Ok(table)
}

pub(crate) fn number_range(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (min, max): (f32, Option<f32>)| {
            Ok(LuaNumberRange(NumberRange {
                min,
                max: max.unwrap_or(min),
            }))
        })?,
    )?;
    Ok(table)
}

pub(crate) fn brick_color(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, value: Value| match &value {
            Value::Integer(number) => Ok(LuaBrickColor::from_number(*number as u32)),
            Value::Number(number) => Ok(LuaBrickColor::from_number(*number as u32)),
            Value::String(text) => {
                let name = text.to_str()?.to_string();
                NAMED
                    .iter()
                    .find(|(candidate, ..)| *candidate == name)
                    .map(|(_, number, r, g, b)| LuaBrickColor {
                        number: *number,
                        color: Some((*r, *g, *b)),
                    })
                    .ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "BrickColor.new: \"{name}\" is not one of the names this build knows"
                        ))
                    })
            }
            other => Err(mlua::Error::runtime(format!(
                "BrickColor.new expected a number or a string, got {}",
                other.type_name()
            ))),
        })?,
    )?;
    Ok(table)
}
