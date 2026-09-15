//! Roblox datatypes exposed to Luau as userdata.
//!
//! Each one maps onto the matching `rbx_dom::Variant` payload, so a value read
//! from the DOM, passed through a script and written back keeps its exact
//! representation.

pub(crate) mod cframe;
pub(crate) mod color3;
pub(crate) mod font;
pub(crate) mod misc;
pub(crate) mod physical_properties;
pub(crate) mod rect;
pub(crate) mod sequence;
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
    Ok(())
}
