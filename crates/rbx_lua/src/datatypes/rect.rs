//! `Rect` datatype.

use mlua::{
    Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic,
};
use rbx_dom::{Rect, Vector2Data};

use super::misc::LuaVector2;
use super::{from_userdata, number_arg};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaRect(pub(crate) Rect);

impl UserData for LuaRect {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Min", |_, this| Ok(LuaVector2(this.0.min)));
        fields.add_field_method_get("Max", |_, this| Ok(LuaVector2(this.0.max)));
        fields.add_field_method_get("Width", |_, this| Ok(this.0.max.x - this.0.min.x));
        fields.add_field_method_get("Height", |_, this| Ok(this.0.max.y - this.0.min.y));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaRect| Ok(*this == other));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "{}, {}, {}, {}",
                this.0.min.x, this.0.min.y, this.0.max.x, this.0.max.y
            ))
        });
    }
}

impl mlua::FromLua for LuaRect {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Rect")
    }
}

fn vector2_arg(value: &Value) -> Option<Vector2Data> {
    if let Value::UserData(data) = value {
        if let Ok(v) = data.borrow::<LuaVector2>() {
            return Some(v.0);
        }
    }
    None
}

const RECT_USAGE: &str = "Rect.new expects (minX, minY, maxX, maxY) or (Vector2, Vector2)";

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, args: Variadic<Value>| match args.as_slice() {
            [a, b] => {
                let min = vector2_arg(a).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                let max = vector2_arg(b).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                Ok(LuaRect(Rect { min, max }))
            }
            [min_x, min_y, max_x, max_y] => {
                let min_x = number_arg(min_x).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                let min_y = number_arg(min_y).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                let max_x = number_arg(max_x).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                let max_y = number_arg(max_y).ok_or_else(|| mlua::Error::runtime(RECT_USAGE))?;
                Ok(LuaRect(Rect {
                    min: Vector2Data { x: min_x, y: min_y },
                    max: Vector2Data { x: max_x, y: max_y },
                }))
            }
            _ => Err(mlua::Error::runtime(RECT_USAGE)),
        })?,
    )?;
    Ok(table)
}
