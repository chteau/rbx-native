//! Generic property bridge: one `__index`/`__newindex` pair for all 2459
//! reflected properties, validated against the reflection database.

mod from_lua;
mod pseudo;
mod to_lua;

use mlua::{Lua, Result, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;

pub(crate) fn missing_instance() -> mlua::Error {
    mlua::Error::runtime("instance has been destroyed")
}

/// Reads a property, returning `Ok(None)` when the class has no such member, so
/// the caller can fall back to a child lookup before erroring out.
pub(crate) fn get(lua: &Lua, ctx: &Ctx, referent: Ref, name: &str) -> Result<Option<Value>> {
    if let Some(value) = pseudo::get(lua, ctx, referent, name)? {
        return Ok(Some(value));
    }

    let (value_type, stored) = {
        let dom = ctx.dom();
        let instance = dom.get(referent).ok_or_else(missing_instance)?;
        let Some(descriptor) = ctx.database().resolve_property(instance.class(), name) else {
            return Ok(None);
        };
        // Whichever name the file stored it under, or the class default:
        // `part.Transparency` on a part a hand-written file left it off of
        // reads `0`, as it would in Roblox.
        let stored = ctx
            .database()
            .stored_or_default(instance, name)
            .map(|(_, value)| value.clone());
        (descriptor.value_type.clone(), stored)
    };

    // Neither stored nor recorded: a value only a running engine computes.
    let Some(stored) = stored else {
        return Ok(Some(Value::Nil));
    };
    to_lua::variant_to_lua(lua, ctx, name, &value_type, &stored).map(Some)
}

/// Writes a property, returning `Ok(false)` when the class has no such member.
pub(crate) fn set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    if pseudo::set(ctx, referent, name, value)? {
        return Ok(true);
    }

    let (value_type, key, existing) = {
        let dom = ctx.dom();
        let instance = dom.get(referent).ok_or_else(missing_instance)?;
        let Some(descriptor) = ctx.database().resolve_property(instance.class(), name) else {
            return Ok(false);
        };
        // Under the name the file stored it as, or else the one Roblox saves
        // it under — `size`, `Color3uint8` — which is what the renderer and
        // the save path read. The default, when there is one, says which
        // representation that name holds.
        let database = ctx.database();
        let (key, existing) = match database.stored_or_default(instance, name) {
            Some((key, value)) => (key.to_owned(), Some(value.clone())),
            None => (
                database.stored_names(instance.class(), name)[0].to_owned(),
                None,
            ),
        };
        (descriptor.value_type.clone(), key, existing)
    };

    let variant = from_lua::lua_to_variant(ctx, name, &value_type, value)?;
    let variant = keep_representation(existing.as_ref(), variant);
    ctx.dom_mut()
        .set_property(referent, &key, variant)
        .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    Ok(true)
}

/// Narrows a freshly built `Variant` to the representation the file already used
/// for that property, so an edited place re-serializes byte-compatibly instead of
/// switching a `Color3uint8` column to `Color3` or a float's width.
fn keep_representation(existing: Option<&Variant>, value: Variant) -> Variant {
    match (existing, &value) {
        (Some(Variant::Color3uint8 { .. }), Variant::Color3(color)) => {
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Variant::Color3uint8 {
                r: channel(color.r),
                g: channel(color.g),
                b: channel(color.b),
            }
        }
        (Some(Variant::Float64(_)), Variant::Float32(v)) => Variant::Float64(*v as f64),
        (Some(Variant::Float32(_)), Variant::Float64(v)) => Variant::Float32(*v as f32),
        (Some(Variant::Int64(_)), Variant::Int32(v)) => Variant::Int64(i64::from(*v)),
        (Some(Variant::Int32(_)), Variant::Int64(v)) => Variant::Int32(*v as i32),
        _ => value,
    }
}
