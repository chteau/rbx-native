//! Luau -> `Variant` conversion, type-checked against the reflected property type.

use mlua::{Result, Value};
use rbx_dom::{Content, PhysicalProperties, Variant};

use crate::ctx::Ctx;
use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::font::LuaFont;
use crate::datatypes::misc::{LuaBrickColor, LuaNumberRange, LuaUDim, LuaUDim2, LuaVector2};
use crate::datatypes::physical_properties::LuaPhysicalProperties;
use crate::datatypes::rect::LuaRect;
use crate::datatypes::sequence::{LuaColorSequence, LuaNumberSequence};
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;

fn type_error(name: &str, expected: &str, value: &Value) -> mlua::Error {
    mlua::Error::runtime(format!(
        "Unable to assign property {name}. {expected} expected, got {}",
        // Luau splits numbers into two value types; scripts only know "number".
        match value {
            Value::Integer(_) => "number",
            other => other.type_name(),
        }
    ))
}

fn number(name: &str, value: &Value) -> Result<f64> {
    match value {
        Value::Number(n) => Ok(*n),
        Value::Integer(i) => Ok(*i as f64),
        other => Err(type_error(name, "number", other)),
    }
}

fn datatype<T: Copy + 'static>(name: &str, expected: &str, value: &Value) -> Result<T> {
    if let Value::UserData(data) = value {
        if let Ok(inner) = data.borrow::<T>() {
            return Ok(*inner);
        }
    }
    Err(type_error(name, expected, value))
}

/// Like `datatype`, for the `Vec`-backed sequence wrappers that cannot be `Copy`.
fn datatype_clone<T: Clone + 'static>(name: &str, expected: &str, value: &Value) -> Result<T> {
    if let Value::UserData(data) = value {
        if let Ok(inner) = data.borrow::<T>() {
            return Ok(inner.clone());
        }
    }
    Err(type_error(name, expected, value))
}

/// `Font` is ambiguous in the reflection database: it names both the newer
/// datatype (`FontFace`) and Roblox's legacy `Enum.Font` (`TextLabel.Font`).
/// The runtime value decides which one a write is targeting.
fn font_or_enum(ctx: &Ctx, name: &str, value: &Value) -> Result<Variant> {
    if let Value::UserData(data) = value {
        if let Ok(font) = data.borrow::<LuaFont>() {
            return Ok(Variant::Font(font.0.clone()));
        }
    }
    enum_variant(ctx, name, "Font", value)
}

pub(crate) fn lua_to_variant(
    ctx: &Ctx,
    name: &str,
    value_type: &str,
    value: &Value,
) -> Result<Variant> {
    match value_type {
        "bool" => match value {
            Value::Boolean(flag) => Ok(Variant::Bool(*flag)),
            other => Err(type_error(name, "boolean", other)),
        },
        "string" | "ProtectedString" | "BinaryString" => match value {
            Value::String(text) => Ok(Variant::String(text.to_str()?.to_string())),
            other => Err(type_error(name, "string", other)),
        },
        "float" => Ok(Variant::Float32(number(name, value)? as f32)),
        "double" => Ok(Variant::Float64(number(name, value)?)),
        "int" => Ok(Variant::Int32(number(name, value)? as i32)),
        "int64" => Ok(Variant::Int64(number(name, value)? as i64)),
        "Vector3" => Ok(Variant::Vector3(
            datatype::<LuaVector3>(name, "Vector3", value)?.0,
        )),
        "Vector2" => Ok(Variant::Vector2(
            datatype::<LuaVector2>(name, "Vector2", value)?.0,
        )),
        "CFrame" => Ok(Variant::CFrame(
            datatype::<LuaCFrame>(name, "CFrame", value)?.0,
        )),
        "Color3" => Ok(Variant::Color3(
            datatype::<LuaColor3>(name, "Color3", value)?.0,
        )),
        "UDim" => Ok(Variant::UDim(datatype::<LuaUDim>(name, "UDim", value)?.0)),
        "UDim2" => Ok(Variant::UDim2(
            datatype::<LuaUDim2>(name, "UDim2", value)?.0,
        )),
        "NumberRange" => Ok(Variant::NumberRange(
            datatype::<LuaNumberRange>(name, "NumberRange", value)?.0,
        )),
        "BrickColor" => Ok(Variant::BrickColor(
            datatype::<LuaBrickColor>(name, "BrickColor", value)?.number(),
        )),
        "NumberSequence" => Ok(Variant::NumberSequence(
            datatype_clone::<LuaNumberSequence>(name, "NumberSequence", value)?.0,
        )),
        "ColorSequence" => Ok(Variant::ColorSequence(
            datatype_clone::<LuaColorSequence>(name, "ColorSequence", value)?.0,
        )),
        "Rect" => Ok(Variant::Rect(datatype::<LuaRect>(name, "Rect", value)?.0)),
        "PhysicalProperties" => match value {
            Value::Nil => Ok(Variant::PhysicalProperties(PhysicalProperties::Default)),
            other => Ok(Variant::PhysicalProperties(
                datatype::<LuaPhysicalProperties>(name, "PhysicalProperties", other)?.0,
            )),
        },
        "Font" => font_or_enum(ctx, name, value),
        "Content" => match value {
            Value::String(text) => Ok(Variant::Content(Content::Uri(text.to_str()?.to_string()))),
            Value::Nil => Ok(Variant::Content(Content::None)),
            other => Err(type_error(name, "Content", other)),
        },
        other => {
            if ctx.database().enum_items(other).is_some() {
                return enum_variant(ctx, name, other, value);
            }
            if ctx.database().class(other).is_some() {
                return reference(name, other, value);
            }
            Err(mlua::Error::runtime(format!(
                "Unable to assign property {name}: type {other} is not supported yet"
            )))
        }
    }
}

fn enum_variant(ctx: &Ctx, name: &str, enum_name: &str, value: &Value) -> Result<Variant> {
    match value {
        Value::UserData(data) => {
            let item = data
                .borrow::<LuaEnumItem>()
                .map_err(|_| type_error(name, &format!("Enum.{enum_name}"), value))?;
            if item.enum_name() != enum_name {
                return Err(type_error(name, &format!("Enum.{enum_name}"), value));
            }
            Ok(Variant::Enum(item.value()))
        }
        // Roblox accepts the ordinal and the item name as well as the EnumItem.
        Value::Integer(ordinal) => Ok(Variant::Enum(*ordinal as u32)),
        Value::Number(ordinal) => Ok(Variant::Enum(*ordinal as u32)),
        Value::String(text) => {
            let item = text.to_str()?.to_string();
            ctx.database()
                .enum_items(enum_name)
                .and_then(|items| items.iter().find(|(candidate, _)| *candidate == item))
                .map(|(_, ordinal)| Variant::Enum(*ordinal))
                .ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "Unable to assign property {name}: {item} is not a valid Enum.{enum_name}"
                    ))
                })
        }
        other => Err(type_error(name, &format!("Enum.{enum_name}"), other)),
    }
}

fn reference(name: &str, class: &str, value: &Value) -> Result<Variant> {
    if let Value::UserData(data) = value {
        if let Ok(instance) = data.borrow::<LuaInstance>() {
            return Ok(Variant::Ref(instance.referent()));
        }
    }
    // `Variant` has no null reference, so clearing an object property would have
    // to erase the entry entirely, which `WeakDom::set_property` cannot express.
    Err(type_error(name, class, value))
}
