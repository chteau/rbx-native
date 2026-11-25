//! `PathWaypoint`: a point of a computed path, how to get there
//! (`Enum.PathWaypointAction`) and the label of what it stands on.

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods,
    UserDataRef, Value,
};

use super::tween_info::enum_item;
use crate::datatypes::vector3::LuaVector3;
use crate::lua_enum::LuaEnumItem;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LuaPathWaypoint {
    pub(crate) position: [f32; 3],
    /// `Walk`, `Jump` or `Custom`.
    pub(crate) action: String,
    pub(crate) label: String,
}

impl UserData for LuaPathWaypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "PathWaypoint");
        fields.add_field_method_get("Position", |_, this| {
            let [x, y, z] = this.position;
            Ok(LuaVector3::new(x, y, z))
        });
        fields.add_field_method_get("Action", |lua, this| {
            enum_item(lua, "PathWaypointAction", &this.action)
        });
        fields.add_field_method_get("Label", |_, this| Ok(this.label.clone()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: UserDataRef<Self>| {
            Ok(*this == *other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let [x, y, z] = this.position;
            Ok(format!("{x}, {y}, {z}, {}, {}", this.action, this.label))
        });
    }
}

pub(super) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (position, action, label): (Option<AnyUserData>, Option<Value>, Option<String>)| {
                let position = match position {
                    Some(v) => {
                        let v = v.borrow::<LuaVector3>()?;
                        [v.0.x, v.0.y, v.0.z]
                    }
                    None => [0.0; 3],
                };
                let action = match action {
                    Some(Value::UserData(item)) => {
                        item.borrow::<LuaEnumItem>()?.item_name().to_owned()
                    }
                    _ => "Walk".to_owned(),
                };
                Ok(LuaPathWaypoint {
                    position,
                    action,
                    label: label.unwrap_or_default(),
                })
            },
        )?,
    )?;
    Ok(table)
}
