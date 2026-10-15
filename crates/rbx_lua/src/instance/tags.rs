//! `Instance:AddTag`/`RemoveTag`/`HasTag`/`GetTags`, over the `Tags`
//! property that `CollectionService` reads too.

use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;

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
