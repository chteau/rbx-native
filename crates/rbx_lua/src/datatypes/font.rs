//! `Font` datatype: the family/weight/style triple behind `FontFace`-style
//! properties (e.g. `TextLabel.FontFace`).
//!
//! Roblox also has a legacy `Enum.Font` (e.g. `TextLabel.Font`) that shares this
//! type name in the reflection database; that ambiguity is resolved where
//! properties are written (`property/from_lua.rs`), not here.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::{Font, FontStyle};

use crate::lua_enum::LuaEnumItem;

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
        Value::UserData(data) if data.borrow::<LuaEnumItem>().is_ok() => {
            let item = data.borrow::<LuaEnumItem>()?;
            Ok(FontStyle::from(item.value() as u8))
        }
        other => Err(mlua::Error::runtime(format!(
            "Font.new expected a style string or number, got {}",
            other.type_name()
        ))),
    }
}

impl UserData for LuaFont {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Font");
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

/// `Font.new`'s weight: an `Enum.FontWeight` item (whose value is the CSS
/// weight, 100–900) or the number itself.
fn weight_from_value(value: &Value) -> Result<u16> {
    match value {
        Value::Integer(weight) => Ok(*weight as u16),
        Value::Number(weight) => Ok(*weight as u16),
        Value::UserData(data) if data.borrow::<LuaEnumItem>().is_ok() => {
            Ok(data.borrow::<LuaEnumItem>()?.value() as u16)
        }
        other => Err(mlua::Error::runtime(format!(
            "Font.new expected an Enum.FontWeight or number, got {}",
            other.type_name()
        ))),
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(
            |_, (family, weight, style): (String, Option<Value>, Option<Value>)| {
                let weight = match weight {
                    None | Some(Value::Nil) => 400,
                    Some(value) => weight_from_value(&value)?,
                };
                let style = match style {
                    Some(value) => style_from_value(&value)?,
                    None => FontStyle::Normal,
                };
                Ok(LuaFont(Font {
                    family,
                    weight,
                    style,
                    cached_face_id: None,
                }))
            },
        )?,
    )?;
    // `fromName`/`fromId` build the family URI `new` takes; `fromEnum` maps
    // a legacy `Enum.Font` onto its family and weight the way Roblox's own
    // table does for the common faces (others keep their own name).
    let more: Table = lua
        .load(
            r#"
            local new = ...
            local families = {
                SourceSans = { "SourceSansPro" }, SourceSansBold = { "SourceSansPro", "Bold" },
                SourceSansLight = { "SourceSansPro", "Light" }, SourceSansSemibold = { "SourceSansPro", "SemiBold" },
                SourceSansItalic = { "SourceSansPro", nil, "Italic" },
                Gotham = { "GothamSSm" }, GothamMedium = { "GothamSSm", "Medium" },
                GothamBold = { "GothamSSm", "Bold" }, GothamBlack = { "GothamSSm", "Heavy" },
                GothamSemibold = { "GothamSSm", "SemiBold" },
                Arial = { "Arimo" }, ArialBold = { "Arimo", "Bold" }, Legacy = { "LegacyArial" },
                Code = { "RobotoMono" }, Highway = { "HighwayGothic" },
                BuilderSans = { "BuilderSans" }, BuilderSansBold = { "BuilderSans", "Bold" },
                BuilderSansMedium = { "BuilderSans", "Medium" }, BuilderSansExtraBold = { "BuilderSans", "ExtraBold" },
            }
            local function family(name) return "rbxasset://fonts/families/" .. name .. ".json" end
            return {
                fromName = function(name, weight, style) return new(family(name), weight, style) end,
                fromId = function(id, weight, style) return new("rbxassetid://" .. tostring(id), weight, style) end,
                fromEnum = function(item)
                    local name = item.Name
                    local known = families[name]
                    if known then
                        return new(
                            family(known[1]),
                            known[2] and Enum.FontWeight[known[2]] or nil,
                            known[3] and Enum.FontStyle[known[3]] or nil
                        )
                    end
                    local bold = name:match("Bold$") and Enum.FontWeight.Bold or nil
                    return new(family((name:gsub("Bold$", ""))), bold)
                end,
            }
            "#,
        )
        .set_name("=Font")
        .call(table.get::<mlua::Function>("new")?)?;
    for pair in more.pairs::<String, Value>() {
        let (key, value) = pair?;
        table.set(key, value)?;
    }
    Ok(table)
}
