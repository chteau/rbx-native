//! Smaller datatypes: `Vector2`, `UDim`, `UDim2`, `NumberRange`, `BrickColor`.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{BrickColor, Color3Data, NumberRange, UDim, UDim2, Vector2Data, DEFAULT_BRICK_COLOR};

use super::color3::LuaColor3;
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

/// One entry of Roblox's `BrickColor` table (see `rbx_dom::BrickColor`),
/// the same table the Properties panel names a part's colour from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LuaBrickColor(pub(crate) &'static BrickColor);

impl LuaBrickColor {
    /// A number the table lacks is "Medium stone grey", as
    /// `BrickColor.new` documents.
    pub(crate) fn from_number(number: u32) -> Self {
        LuaBrickColor(
            BrickColor::from_number(number)
                .or_else(|| BrickColor::from_number(DEFAULT_BRICK_COLOR))
                .expect("the table holds the default"),
        )
    }

    pub(crate) fn number(&self) -> u32 {
        self.0.number
    }
}

impl UserData for LuaBrickColor {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Number", |_, this| Ok(this.0.number));
        fields.add_field_method_get("Name", |_, this| Ok(this.0.name));
        fields.add_field_method_get("Color", |_, this| {
            let [r, g, b] = this.0.rgb.map(|channel| f32::from(channel) / 255.0);
            Ok(LuaColor3(Color3Data { r, g, b }))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaBrickColor| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0.name));
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
        lua.create_function(|_, args: mlua::MultiValue| {
            let args: Vec<Value> = args.into_iter().collect();
            match args.as_slice() {
                // Three 0-1 channels: the closest colour to them.
                [r, g, b] => Ok(LuaBrickColor(BrickColor::nearest([
                    channel_arg(r)?,
                    channel_arg(g)?,
                    channel_arg(b)?,
                ]))),
                [Value::Integer(number)] => Ok(LuaBrickColor::from_number(*number as u32)),
                [Value::Number(number)] => Ok(LuaBrickColor::from_number(*number as u32)),
                // An unknown name is "Medium stone grey" too.
                [Value::String(text)] => Ok(BrickColor::from_name(&text.to_str()?)
                    .map(LuaBrickColor)
                    .unwrap_or_else(|| LuaBrickColor::from_number(DEFAULT_BRICK_COLOR))),
                [Value::UserData(data)] => {
                    let color = data
                        .borrow::<LuaColor3>()
                        .map_err(|_| mlua::Error::runtime("BrickColor.new expected a Color3"))?;
                    Ok(LuaBrickColor(BrickColor::nearest(
                        [color.0.r, color.0.g, color.0.b].map(channel),
                    )))
                }
                other => Err(mlua::Error::runtime(format!(
                    "BrickColor.new expected a number, a name, a Color3 or three numbers,                      got {} arguments",
                    other.len()
                ))),
            }
        })?,
    )?;
    table.set(
        "palette",
        lua.create_function(|_, index: u8| {
            BrickColor::from_palette(index)
                .map(LuaBrickColor)
                .ok_or_else(|| mlua::Error::runtime("BrickColor.palette: index out of range"))
        })?,
    )?;
    Ok(table)
}

/// A 0-1 channel as the byte the table's colours are compared in.
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn channel_arg(value: &Value) -> Result<u8> {
    number_arg(value)
        .map(channel)
        .ok_or_else(|| mlua::Error::runtime("BrickColor.new expected numbers"))
}
