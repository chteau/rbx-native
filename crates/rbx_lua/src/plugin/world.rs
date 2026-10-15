//! World-facing services (`TextService`, `TweenService`,
//! `MarketplaceService`) and the spatial queries on `Workspace`.

mod legacy;
mod raycast;
#[cfg(test)]
mod tests;
mod text;
mod tween;

use mlua::{Lua, MetaMethod, Result, UserData, UserDataMethods, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;

struct Marketplace;

impl UserData for Marketplace {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, _, _key: String| {
            lua.create_function(|_, _: mlua::MultiValue| -> Result<()> {
                Err(mlua::Error::runtime(
                    "MarketplaceService is not available in rbx-native",
                ))
            })
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("MarketplaceService"));
    }
}

/// Services the plugin host implements itself; the names it returns `None`
/// for are real instances of the place.
pub(crate) fn service(lua: &Lua, name: &str) -> Result<Option<Value>> {
    if !matches!(name, "TextService" | "TweenService" | "MarketplaceService") {
        return Ok(None);
    }
    let key = format!("rbxn.service.{name}");
    if let Some(cached) = lua.named_registry_value::<Option<Value>>(&key)? {
        return Ok(Some(cached));
    }
    let value = match name {
        "TextService" => Value::UserData(lua.create_userdata(text::TextService)?),
        "TweenService" => Value::UserData(lua.create_userdata(tween::TweenService)?),
        _ => Value::UserData(lua.create_userdata(Marketplace)?),
    };
    lua.set_named_registry_value(&key, value.clone())?;
    Ok(Some(value))
}

/// `Workspace:Raycast`, `FindPartsInRegion3`, `GetPartBoundsInBox` and
/// `GetPartsInPart`; `None` for any other member.
pub(crate) fn class_member(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    if let Some(value) = legacy::member(lua, ctx, referent, class, key)? {
        return Ok(Some(value));
    }
    if class != "Workspace" {
        return Ok(None);
    }
    raycast::member(lua, ctx, referent, key)
}
