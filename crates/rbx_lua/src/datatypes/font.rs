//! `Font` datatype: the family/weight/style triple behind `FontFace`-style
//! properties (e.g. `TextLabel.FontFace`).
//!
//! Roblox also has a legacy `Enum.Font` (e.g. `TextLabel.Font`) that shares this
//! type name in the reflection database; that ambiguity is resolved where
//! properties are written (`property/from_lua.rs`), not here.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{Font, FontStyle};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LuaFont(pub(crate) Font);

fn style_name(style: FontStyle) -> String {
    match style {
        FontStyle::Normal => "Normal".to_string(),
        FontStyle::Italic => "Italic".to_string(),
        FontStyle::Other(ordinal) => format!("Other({ordinal})"),
    }
}

fn style_from_value(value: &Value) -> Result<FontStyle> {
    match value {
        Value::String(text) => match text.to_str()?.as_ref() {
            "Normal" => Ok(FontStyle::Normal),
            "Italic" => Ok(FontStyle::Italic),
            other => Err(mlua::Error::runtime(format!(
                "Font.new: unknown style \"{other}\", expected \"Normal\" or \"Italic\""
            ))),
        },
        Value::Integer(ordinal) => Ok(FontStyle::from(*ordinal as u8)),
        Value::Number(ordinal) => Ok(FontStyle::from(*ordinal as u8)),
        other => Err(mlua::Error::runtime(format!(
            "Font.new expected a style string or number, got {}",
            other.type_name()
        ))),
    }
}

impl UserData for LuaFont {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("Family", |_, this| Ok(this.0.family.clone()));
        fields.add_field_method_get("Weight", |_, this| Ok(i32::from(this.0.weight)));
        fields.add_field_method_get("Style", |_, this| Ok(style_name(this.0.style)));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaFont| Ok(*this == other));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.0.family.clone())
        });
    }
}

impl mlua::FromLua for LuaFont {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        if let Value::UserData(data) = &value {
            if let Ok(inner) = data.borrow::<Self>() {
                return Ok(inner.clone());
            }
        }
        Err(mlua::Error::runtime(format!(
            "Font expected, got {}",
            value.type_name()
        )))
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (family, weight, style): (String, Option<i32>, Option<Value>)| {
                let style = match style {
                    Some(value) => style_from_value(&value)?,
                    None => FontStyle::Normal,
                };
                Ok(LuaFont(Font {
                    family,
                    weight: weight.unwrap_or(400) as u16,
                    style,
                    cached_face_id: None,
                }))
            },
        )?,
    )?;
    Ok(table)
}
