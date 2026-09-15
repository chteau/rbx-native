//! `Variant` -> Luau conversion.

use mlua::{IntoLua, Lua, Result, Value};
use rbx_dom::{Color3Data, Content, PhysicalProperties, Variant};

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

pub(crate) fn variant_to_lua(
    lua: &Lua,
    ctx: &Ctx,
    name: &str,
    value_type: &str,
    variant: &Variant,
) -> Result<Value> {
    match variant {
        Variant::String(value) => value.clone().into_lua(lua),
        Variant::Bool(value) => (*value).into_lua(lua),
        Variant::Int32(value) => (*value).into_lua(lua),
        Variant::Int64(value) => (*value).into_lua(lua),
        Variant::Float32(value) => (*value).into_lua(lua),
        Variant::Float64(value) => (*value).into_lua(lua),
        Variant::Vector3(value) => LuaVector3(*value).into_lua(lua),
        Variant::Vector2(value) => LuaVector2(*value).into_lua(lua),
        Variant::CFrame(value) => LuaCFrame(*value).into_lua(lua),
        Variant::OptionalCFrame(value) => match value {
            Some(frame) => LuaCFrame(*frame).into_lua(lua),
            None => Ok(Value::Nil),
        },
        Variant::Color3(value) => LuaColor3(*value).into_lua(lua),
        // The file stores 8-bit channels for `Color`, scripts see the 0..1 Color3.
        Variant::Color3uint8 { r, g, b } => LuaColor3(Color3Data {
            r: f32::from(*r) / 255.0,
            g: f32::from(*g) / 255.0,
            b: f32::from(*b) / 255.0,
        })
        .into_lua(lua),
        Variant::BrickColor(value) => LuaBrickColor::from_number(*value).into_lua(lua),
        Variant::UDim(value) => LuaUDim(*value).into_lua(lua),
        Variant::UDim2(value) => LuaUDim2(*value).into_lua(lua),
        Variant::NumberRange(value) => LuaNumberRange(*value).into_lua(lua),
        Variant::Enum(value) => match ctx.database().enum_name(value_type, *value) {
            Some(item) => LuaEnumItem::new(
                value_type.to_string(),
                item.to_string(),
                *value,
                ctx.clone(),
            )
            .into_lua(lua),
            // Enum ordinals absent from the dump stay visible as raw numbers
            // rather than failing the whole read.
            None => (*value).into_lua(lua),
        },
        Variant::Ref(referent) => {
            if ctx.dom().get(*referent).is_none() {
                return Ok(Value::Nil);
            }
            LuaInstance::new(*referent, ctx.clone()).into_lua(lua)
        }
        Variant::NumberSequence(value) => LuaNumberSequence(value.clone()).into_lua(lua),
        Variant::ColorSequence(value) => LuaColorSequence(value.clone()).into_lua(lua),
        Variant::Rect(value) => LuaRect(*value).into_lua(lua),
        // Roblox itself reads a `Default` physics property as `nil`, not an object.
        Variant::PhysicalProperties(PhysicalProperties::Default) => Ok(Value::Nil),
        Variant::PhysicalProperties(custom) => LuaPhysicalProperties(*custom).into_lua(lua),
        Variant::Font(value) => LuaFont(value.clone()).into_lua(lua),
        Variant::Content(value) => match value {
            Content::None => String::new().into_lua(lua),
            Content::Uri(uri) => uri.clone().into_lua(lua),
            // Internal instance references have no URL form; scripts rarely read
            // this shape, so it is exposed as empty rather than left unsupported.
            Content::Object(_) => String::new().into_lua(lua),
        },
        other => Err(mlua::Error::runtime(format!(
            "reading {name} is not supported yet: {} has no Luau representation",
            variant_kind(other)
        ))),
    }
}

fn variant_kind(variant: &Variant) -> &'static str {
    match variant {
        Variant::Ray { .. } => "Ray",
        Variant::Faces(_) => "Faces",
        Variant::Axes(_) => "Axes",
        Variant::Vector3int16 { .. } => "Vector3int16",
        Variant::SharedString(_) => "SharedString",
        Variant::UniqueId(_) => "UniqueId",
        Variant::SecurityCapabilities(_) => "SecurityCapabilities",
        _ => "this property type",
    }
}
