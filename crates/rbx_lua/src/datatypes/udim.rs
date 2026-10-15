//! `UDim` and `UDim2`. The globals are installed by `misc` (via `udim`/`udim2`).

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{UDim, UDim2};

use super::{from_userdata, number_arg};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaUDim(pub(crate) UDim);

fn add_udim(a: UDim, b: UDim) -> UDim {
    UDim {
        scale: a.scale + b.scale,
        offset: a.offset + b.offset,
    }
}

fn sub_udim(a: UDim, b: UDim) -> UDim {
    UDim {
        scale: a.scale - b.scale,
        offset: a.offset - b.offset,
    }
}

fn neg_udim(a: UDim) -> UDim {
    UDim {
        scale: -a.scale,
        offset: -a.offset,
    }
}

fn lerp_udim(a: UDim, b: UDim, t: f32) -> UDim {
    UDim {
        scale: a.scale + (b.scale - a.scale) * t,
        offset: (a.offset as f32 + (b.offset - a.offset) as f32 * t).round() as i32,
    }
}

impl UserData for LuaUDim {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Scale", |_, this| Ok(this.0.scale));
        fields.add_field_method_get("Offset", |_, this| Ok(this.0.offset));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaUDim| Ok(*this == other));
        methods.add_meta_method(MetaMethod::Add, |_, this, o: LuaUDim| {
            Ok(LuaUDim(add_udim(this.0, o.0)))
        });
        methods.add_meta_method(MetaMethod::Sub, |_, this, o: LuaUDim| {
            Ok(LuaUDim(sub_udim(this.0, o.0)))
        });
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| Ok(LuaUDim(neg_udim(this.0))));
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
        fields.add_field_method_get("Width", |_, this| Ok(LuaUDim(this.0.x)));
        fields.add_field_method_get("Height", |_, this| Ok(LuaUDim(this.0.y)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: LuaUDim2| Ok(*this == other),
        );
        methods.add_meta_method(MetaMethod::Add, |_, this, o: LuaUDim2| {
            Ok(LuaUDim2(UDim2 {
                x: add_udim(this.0.x, o.0.x),
                y: add_udim(this.0.y, o.0.y),
            }))
        });
        methods.add_meta_method(MetaMethod::Sub, |_, this, o: LuaUDim2| {
            Ok(LuaUDim2(UDim2 {
                x: sub_udim(this.0.x, o.0.x),
                y: sub_udim(this.0.y, o.0.y),
            }))
        });
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| {
            Ok(LuaUDim2(UDim2 {
                x: neg_udim(this.0.x),
                y: neg_udim(this.0.y),
            }))
        });
        methods.add_method("Lerp", |_, this, (goal, alpha): (LuaUDim2, f32)| {
            Ok(LuaUDim2(UDim2 {
                x: lerp_udim(this.0.x, goal.0.x, alpha),
                y: lerp_udim(this.0.y, goal.0.y, alpha),
            }))
        });
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
        lua.create_function(|_, args: mlua::Variadic<Value>| {
            // `UDim2.new(xUDim, yUDim)` as well as the four-number form.
            if let (Some(Value::UserData(x)), Some(Value::UserData(y))) =
                (args.first(), args.get(1))
            {
                if let (Ok(x), Ok(y)) = (x.borrow::<LuaUDim>(), y.borrow::<LuaUDim>()) {
                    return Ok(LuaUDim2(UDim2 { x: x.0, y: y.0 }));
                }
            }
            let number = |index: usize| -> Result<f32> {
                match args.get(index) {
                    None | Some(Value::Nil) => Ok(0.0),
                    Some(value) => number_arg(value).ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "UDim2.new: argument #{} expected a number, got {}",
                            index + 1,
                            value.type_name()
                        ))
                    }),
                }
            };
            // Roblox truncates a fractional offset rather than refusing it.
            Ok(LuaUDim2(UDim2 {
                x: UDim {
                    scale: number(0)?,
                    offset: number(1)? as i32,
                },
                y: UDim {
                    scale: number(2)?,
                    offset: number(3)? as i32,
                },
            }))
        })?,
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

pub(crate) fn register(_lua: &Lua) -> Result<()> {
    Ok(())
}
