//! `Instance:AddTag`/`RemoveTag`/`HasTag`/`GetTags`, over the `Tags`
//! property that `CollectionService` reads too.

use mlua::{Lua, MultiValue, Value};
use rbx_dom::{Ref, Variant};

use super::LuaInstance;
use crate::ctx::Ctx;
use crate::engine::{collection, Engine};

pub(crate) fn get(ctx: &Ctx, referent: Ref) -> Vec<String> {
    ctx.dom()
        .get(referent)
        .map(|i| {
            rbx_dom::attributes::tags(i.properties().get("Tags"))
                .into_iter()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Adds (`add`) or removes `tag`; `false` when that changed nothing.
pub(crate) fn set(ctx: &Ctx, referent: Ref, tag: &str, add: bool) -> mlua::Result<bool> {
    let mut tags = get(ctx, referent);
    let had = tags.iter().any(|t| t == tag);
    if had == add {
        return Ok(false);
    }
    ctx.check_write(referent)?;
    if add {
        tags.push(tag.to_string());
    } else {
        tags.retain(|t| t != tag);
    }
    let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    ctx.dom_mut()
        .set_property(
            referent,
            "Tags",
            Variant::String(rbx_dom::attributes::encode_tags(&refs)),
        )
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
    Ok(true)
}

/// `CollectionService:AddTag(instance, tag)` and its siblings: the service
/// is an instance too, so these calls reach the instance methods of the
/// same name first, which hand them on when `receiver` is the service and
/// the first argument an instance. `None` for a plain instance call.
pub(crate) fn service_call(
    lua: &Lua,
    ctx: &Ctx,
    receiver: Ref,
    name: &str,
    args: &MultiValue,
) -> mlua::Result<Option<MultiValue>> {
    let is_service = ctx
        .dom()
        .get(receiver)
        .is_some_and(|i| i.class() == "CollectionService");
    let names_instance = matches!(
        args.front(),
        Some(Value::UserData(data)) if data.is::<LuaInstance>()
    );
    if !is_service || !names_instance || lua.app_data_ref::<std::rc::Rc<Engine>>().is_none() {
        return Ok(None);
    }
    let Some(Value::Function(method)) = collection::member(lua, name)? else {
        return Ok(None);
    };
    let mut call = args.clone();
    call.push_front(Value::Nil);
    method.call::<MultiValue>(call).map(Some)
}
