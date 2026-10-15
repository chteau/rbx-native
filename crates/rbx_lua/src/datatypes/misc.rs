//! Smaller datatypes: `Vector2`, `UDim`, `UDim2`, `NumberRange`, `BrickColor`.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{BrickColor, Color3Data, NumberRange, Vector2Data, DEFAULT_BRICK_COLOR};

use super::color3::LuaColor3;
use super::{from_userdata, number_arg};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaVector2(pub(crate) Vector2Data);

impl LuaVector2 {
    fn new(x: f32, y: f32) -> Self {
        LuaVector2(Vector2Data { x, y })
    }
}

fn vec2_of(value: &Value) -> Option<LuaVector2> {
    match value {
        Value::UserData(data) => data.borrow::<LuaVector2>().ok().map(|v| *v),
        _ => None,
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
        // Roblox still answers to the deprecated lowercase spellings.
        for name in ["X", "x"] {
            fields.add_field_method_get(name, |_, this| Ok(this.0.x));
        }
        for name in ["Y", "y"] {
            fields.add_field_method_get(name, |_, this| Ok(this.0.y));
        }
        for name in ["Magnitude", "magnitude"] {
            fields.add_field_method_get(name, |_, this| {
                Ok((this.0.x * this.0.x + this.0.y * this.0.y).sqrt())
            });
        }
        for name in ["Unit", "unit"] {
            fields.add_field_method_get(name, |_, this| {
                let m = (this.0.x * this.0.x + this.0.y * this.0.y).sqrt();
                Ok(LuaVector2::new(this.0.x / m, this.0.y / m))
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Add, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a + b)
        });
        methods.add_meta_method(MetaMethod::Sub, |_, this, rhs: Value| {
            combine2(this, &rhs, |a, b| a - b)
        });
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            match (vec2_of(&a), vec2_of(&b)) {
                (Some(lhs), _) => combine2(&lhs, &b, |x, y| x * y),
                (None, Some(rhs)) => combine2(&rhs, &a, |x, y| x * y),
                _ => Err(mlua::Error::runtime(
                    "Vector2 expected a Vector2 or a number",
                )),
            }
        });
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| {
            Ok(LuaVector2::new(-this.0.x, -this.0.y))
        });
        methods.add_method("Dot", |_, this, o: LuaVector2| {
            Ok(this.0.x * o.0.x + this.0.y * o.0.y)
        });
        methods.add_method("Cross", |_, this, o: LuaVector2| {
            Ok(this.0.x * o.0.y - this.0.y * o.0.x)
        });
        methods.add_method("Lerp", |_, this, (o, t): (LuaVector2, f32)| {
            Ok(LuaVector2::new(
                this.0.x + (o.0.x - this.0.x) * t,
                this.0.y + (o.0.y - this.0.y) * t,
            ))
        });
        methods.add_method("Abs", |_, this, ()| {
            Ok(LuaVector2::new(this.0.x.abs(), this.0.y.abs()))
        });
        methods.add_method("Floor", |_, this, ()| {
            Ok(LuaVector2::new(this.0.x.floor(), this.0.y.floor()))
        });
        methods.add_method("Ceil", |_, this, ()| {
            Ok(LuaVector2::new(this.0.x.ceil(), this.0.y.ceil()))
        });
        methods.add_method("Sign", |_, this, ()| {
            let sign = |v: f32| if v == 0.0 { 0.0 } else { v.signum() };
            Ok(LuaVector2::new(sign(this.0.x), sign(this.0.y)))
        });
        methods.add_method("Min", |_, this, others: mlua::Variadic<LuaVector2>| {
            Ok(others.iter().fold(*this, |acc, o| {
                LuaVector2::new(acc.0.x.min(o.0.x), acc.0.y.min(o.0.y))
            }))
        });
        methods.add_method("Max", |_, this, others: mlua::Variadic<LuaVector2>| {
            Ok(others.iter().fold(*this, |acc, o| {
                LuaVector2::new(acc.0.x.max(o.0.x), acc.0.y.max(o.0.y))
            }))
        });
        methods.add_method("FuzzyEq", |_, this, (o, eps): (LuaVector2, Option<f32>)| {
            let eps = eps.unwrap_or(1e-5);
            Ok(
                (this.0.x - o.0.x).abs() <= eps * (1.0 + this.0.x.abs().max(o.0.x.abs()))
                    && (this.0.y - o.0.y).abs() <= eps * (1.0 + this.0.y.abs().max(o.0.y.abs())),
            )
        });
        methods.add_method(
            "Angle",
            |_, this, (o, signed): (LuaVector2, Option<bool>)| {
                let dot = this.0.x * o.0.x + this.0.y * o.0.y;
                let cross = this.0.x * o.0.y - this.0.y * o.0.x;
                let angle = cross.atan2(dot);
                Ok(if signed.unwrap_or(false) {
                    angle
                } else {
                    angle.abs()
                })
            },
        );
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

pub(crate) use super::udim::{udim, udim2, LuaUDim, LuaUDim2};

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
    table.set("zero", LuaVector2::new(0.0, 0.0))?;
    table.set("one", LuaVector2::new(1.0, 1.0))?;
    table.set("xAxis", LuaVector2::new(1.0, 0.0))?;
    table.set("yAxis", LuaVector2::new(0.0, 1.0))?;
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
                    "BrickColor.new expected a number, a name, a Color3 or three numbers, \
                     got {} arguments",
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
