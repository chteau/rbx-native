//! `TweenInfo` datatype.
//!
//! Easing settings are stored by name and resolved through the `Enum` global on
//! read, so they come back as real enum items.

use mlua::{
    Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataFields, UserDataMethods,
    UserDataRef, Value,
};

use crate::lua_enum::LuaEnumItem;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LuaTweenInfo {
    pub(crate) time: f32,
    pub(crate) delay: f32,
    pub(crate) repeat_count: i32,
    pub(crate) reverses: bool,
    pub(crate) style: String,
    pub(crate) direction: String,
}

fn enum_item(lua: &Lua, enum_name: &str, item: &str) -> Result<Value> {
    let lookup = |value: Value, key: &str| -> Result<Value> {
        match value {
            Value::Table(table) => table.get(key),
            Value::UserData(data) => data.get(key),
            other => Err(mlua::Error::runtime(format!(
                "Enum lookup failed: {}",
                other.type_name()
            ))),
        }
    };
    let root: Value = lua.globals().get("Enum")?;
    lookup(lookup(root, enum_name)?, item)
}

fn enum_arg(value: Option<Value>, expected: &str, default: &str) -> Result<String> {
    match value {
        None | Some(Value::Nil) => Ok(default.to_string()),
        Some(Value::UserData(data)) => {
            let matches = data
                .borrow::<LuaEnumItem>()
                .map(|item| item.enum_name() == expected)
                .unwrap_or(false);
            if !matches {
                return Err(mlua::Error::runtime(format!(
                    "TweenInfo expected an Enum.{expected} item"
                )));
            }
            data.get("Name")
        }
        Some(other) => Err(mlua::Error::runtime(format!(
            "Enum.{expected} expected, got {}",
            other.type_name()
        ))),
    }
}

impl UserData for LuaTweenInfo {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "TweenInfo");
        fields.add_field_method_get("Time", |_, this| Ok(this.time));
        fields.add_field_method_get("DelayTime", |_, this| Ok(this.delay));
        fields.add_field_method_get("RepeatCount", |_, this| Ok(this.repeat_count));
        fields.add_field_method_get("Reverses", |_, this| Ok(this.reverses));
        fields.add_field_method_get("EasingStyle", |lua, this| {
            enum_item(lua, "EasingStyle", &this.style)
        });
        fields.add_field_method_get("EasingDirection", |lua, this| {
            enum_item(lua, "EasingDirection", &this.direction)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: UserDataRef<Self>| {
            Ok(*this == *other)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "{}, {}, {}, {}, {}, {}",
                this.time, this.style, this.direction, this.repeat_count, this.reverses, this.delay
            ))
        });
    }
}

type NewArgs = (
    Option<f32>,
    Option<Value>,
    Option<Value>,
    Option<i32>,
    Option<bool>,
    Option<f32>,
);

pub(super) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (time, style, direction, repeat_count, reverses, delay): NewArgs| {
                Ok(LuaTweenInfo {
                    time: time.unwrap_or(1.0),
                    delay: delay.unwrap_or(0.0),
                    repeat_count: repeat_count.unwrap_or(0),
                    reverses: reverses.unwrap_or(false),
                    style: enum_arg(style, "EasingStyle", "Quad")?,
                    direction: enum_arg(direction, "EasingDirection", "Out")?,
                })
            },
        )?,
    )?;
    Ok(table)
}
