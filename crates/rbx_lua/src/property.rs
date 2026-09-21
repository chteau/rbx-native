//! Generic property bridge: one `__index`/`__newindex` pair for all 2459
//! reflected properties, validated against the reflection database.

mod from_lua;
mod pseudo;
mod to_lua;

use mlua::{Lua, Result, Value};
use rbx_dom::{Instance, Ref, Variant};
use rbx_reflection::{PropertyDescriptor, ReflectionDatabase};

use crate::ctx::Ctx;

/// Properties whose serialized name differs from the name scripts use.
///
/// The API dump carries no serialization info, so the file's spelling is resolved
/// here. The generic rule below (lowercase first letter, as in `Size` -> `size`)
/// covers most of them; this table holds what it cannot derive.
const ALIASES: &[(&str, &str)] = &[("Color", "Color3uint8")];

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
        let key = storage_key(instance, name);
        (
            descriptor.value_type.clone(),
            instance.properties().get(&key).cloned(),
        )
    };

    // A property the file never stored has no default here: the dump records types,
    // not default values.
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
        let key = storage_key(instance, name);
        let existing = instance.properties().get(&key).cloned();
        (descriptor.value_type.clone(), key, existing)
    };

    let variant = from_lua::lua_to_variant(ctx, name, &value_type, value)?;
    let variant = keep_representation(existing.as_ref(), variant);
    ctx.dom_mut()
        .set_property(referent, &key, variant)
        .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    Ok(true)
}

fn storage_key(instance: &Instance, canonical: &str) -> String {
    if instance.properties().contains_key(canonical) {
        return canonical.to_string();
    }

    let mut chars = canonical.chars();
    let lowered = match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    if instance.properties().contains_key(&lowered) {
        return lowered;
    }

    ALIASES
        .iter()
        .find(|(name, alias)| *name == canonical && instance.properties().contains_key(*alias))
        .map(|(_, alias)| alias.to_string())
        .unwrap_or_else(|| canonical.to_string())
}

/// The reflected property that `key`, as a file stores it on an instance of
/// `class`, stands for: `storage_key` read backwards, through the same rules
/// and the same alias table, so `size` is `Part.Size` and `Color3uint8` is
/// `BasePart.Color`. `None` for a key the dump has no property for at all —
/// `Tags`, `AttributesSerialize` and the other serialized-only data.
pub fn reflected_property<'a>(
    database: &'a ReflectionDatabase,
    class: &str,
    key: &str,
) -> Option<&'a PropertyDescriptor> {
    let resolve = |name: &str| database.resolve_property(class, name);
    let mut chars = key.chars();
    let raised = chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str());
    resolve(key)
        .or_else(|| raised.as_deref().and_then(resolve))
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(_, alias)| *alias == key)
                .and_then(|(name, _)| resolve(name))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_key_reads_back_as_the_property_scripts_name() {
        let database = ReflectionDatabase::embedded();
        let name = |class: &str, key: &str| {
            reflected_property(&database, class, key).map(|property| property.name.as_str())
        };
        assert_eq!(name("Part", "size"), Some("Size"));
        assert_eq!(name("Part", "shape"), Some("Shape"));
        assert_eq!(name("Part", "Color3uint8"), Some("Color"));
        assert_eq!(name("Part", "CFrame"), Some("CFrame"));
        // Declared on `Part`, so a `MeshPart` has no such property to map to.
        assert_eq!(name("MeshPart", "shape"), None);
        assert_eq!(name("Part", "Tags"), None);
        assert_eq!(name("Part", "AttributesSerialize"), None);
    }
}
