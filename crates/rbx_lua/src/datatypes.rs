//! Roblox datatypes exposed to Luau as userdata.
//!
//! Each one maps onto the matching `rbx_dom::Variant` payload, so a value read
//! from the DOM, passed through a script and written back keeps its exact
//! representation.

pub(crate) mod cframe;
pub(crate) mod color3;
pub(crate) mod extra;
pub(crate) mod font;
pub(crate) mod int16;
pub(crate) mod misc;
pub(crate) mod params;
pub(crate) mod physical_properties;
pub(crate) mod rect;
pub(crate) mod sequence;
pub(crate) mod udim;
pub(crate) mod vector3;

use mlua::{Lua, Result, Value};

/// Accepts both Luau number representations, since integers and floats are
/// distinct `Value` variants but interchangeable as datatype components.
pub(crate) fn number_arg(value: &Value) -> Option<f32> {
    match value {
        Value::Number(n) => Some(*n as f32),
        Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

pub(crate) fn from_userdata<T: Copy + 'static>(value: &Value, expected: &str) -> Result<T> {
    if let Value::UserData(data) = value {
        if let Ok(inner) = data.borrow::<T>() {
            return Ok(*inner);
        }
    }
    Err(mlua::Error::runtime(format!(
        "{expected} expected, got {}",
        value.type_name()
    )))
}

pub(crate) fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    globals.set("Vector3", vector3::constructors(lua)?)?;
    globals.set("CFrame", cframe::constructors(lua)?)?;
    globals.set("Color3", color3::constructors(lua)?)?;
    globals.set("Vector2", misc::vector2(lua)?)?;
    globals.set("UDim", misc::udim(lua)?)?;
    globals.set("UDim2", misc::udim2(lua)?)?;
    globals.set("NumberRange", misc::number_range(lua)?)?;
    globals.set("BrickColor", misc::brick_color(lua)?)?;
    globals.set("Rect", rect::constructors(lua)?)?;
    globals.set(
        "PhysicalProperties",
        physical_properties::constructors(lua)?,
    )?;
    globals.set("Font", font::constructors(lua)?)?;
    globals.set("NumberSequence", sequence::number_sequence(lua)?)?;
    globals.set(
        "NumberSequenceKeypoint",
        sequence::number_sequence_keypoint(lua)?,
    )?;
    globals.set("ColorSequence", sequence::color_sequence(lua)?)?;
    globals.set(
        "ColorSequenceKeypoint",
        sequence::color_sequence_keypoint(lua)?,
    )?;
    extra::register(lua)?;
    params::register(lua)?;
    udim::register(lua)?;
    int16::register(lua)?;
    lowercase_constructors(lua)
}

/// `Vector3.fromNormalId`, `CFrame.fromAxisAngle`…: Roblox answers to the
/// constructors' lowercase-first spellings too.
fn lowercase_constructors(lua: &Lua) -> Result<()> {
    for global in ["Vector3", "Vector2", "CFrame", "Color3", "UDim2"] {
        let table: mlua::Table = lua.globals().get(global)?;
        let mut aliases = Vec::new();
        for pair in table.pairs::<String, Value>() {
            let (key, value) = pair?;
            let mut chars = key.chars();
            if let Some(first) = chars.next().filter(char::is_ascii_uppercase) {
                let alias: String = first.to_ascii_lowercase().to_string() + chars.as_str();
                aliases.push((alias, value));
            }
        }
        for (alias, value) in aliases {
            if table.get::<Value>(alias.as_str())?.is_nil() {
                table.set(alias, value)?;
            }
        }
    }
    Ok(())
}
