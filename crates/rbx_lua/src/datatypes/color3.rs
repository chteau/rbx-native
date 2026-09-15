//! `Color3` datatype.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::Color3Data;

use super::from_userdata;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaColor3(pub(crate) Color3Data);

impl LuaColor3 {
    pub(crate) fn new(r: f32, g: f32, b: f32) -> Self {
        LuaColor3(Color3Data { r, g, b })
    }
}

impl UserData for LuaColor3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("R", |_, this| Ok(this.0.r));
        fields.add_field_method_get("G", |_, this| Ok(this.0.g));
        fields.add_field_method_get("B", |_, this| Ok(this.0.b));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ToHex", |_, this, ()| {
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Ok(format!(
                "{:02x}{:02x}{:02x}",
                channel(this.0.r),
                channel(this.0.g),
                channel(this.0.b)
            ))
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaColor3| {
            Ok(*this == other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.r, this.0.g, this.0.b))
        });
    }
}

impl mlua::FromLua for LuaColor3 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Color3")
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3::new(
                r.unwrap_or_default(),
                g.unwrap_or_default(),
                b.unwrap_or_default(),
            ))
        })?,
    )?;
    table.set(
        "fromRGB",
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3::new(
                r.unwrap_or_default() / 255.0,
                g.unwrap_or_default() / 255.0,
                b.unwrap_or_default() / 255.0,
            ))
        })?,
    )?;
    Ok(table)
}
