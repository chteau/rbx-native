//! `Path2DControlPoint`: a point of a `Path2D` and its two tangents, each
//! a `UDim2` (the tangents relative to the point).

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods};
use rbx_dom::{UDim, UDim2};

use crate::datatypes::udim::LuaUDim2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaPath2DControlPoint {
    pub(crate) position: UDim2,
    pub(crate) left: UDim2,
    pub(crate) right: UDim2,
}

impl UserData for LuaPath2DControlPoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Path2DControlPoint");
        fields.add_field_method_get("Position", |_, this| Ok(LuaUDim2(this.position)));
        fields.add_field_method_get("LeftTangent", |_, this| Ok(LuaUDim2(this.left)));
        fields.add_field_method_get("RightTangent", |_, this| Ok(LuaUDim2(this.right)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<Self>| {
            Ok(*this == *other)
        });
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            let text = |u: UDim2| -> Result<String> {
                lua.globals()
                    .get::<mlua::Function>("tostring")?
                    .call(LuaUDim2(u))
            };
            Ok(format!(
                "Path2DControlPoint {{ Position = {}, LeftTangent = {}, RightTangent = {} }}",
                text(this.position)?,
                text(this.left)?,
                text(this.right)?
            ))
        });
    }
}

pub(super) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (position, left, right): (Option<LuaUDim2>, Option<LuaUDim2>, Option<LuaUDim2>)| {
                let none = UDim {
                    scale: 0.0,
                    offset: 0,
                };
                let zero = LuaUDim2(UDim2 { x: none, y: none });
                Ok(LuaPath2DControlPoint {
                    position: position.unwrap_or(zero).0,
                    left: left.unwrap_or(zero).0,
                    right: right.unwrap_or(zero).0,
                })
            },
        )?,
    )?;
    Ok(table)
}
