//! Placeholder, filled in by its own task.

use mlua::{Lua, Result, Value};
use rbx_dom::Ref;

use crate::ctx::Ctx;

pub(crate) fn service(_lua: &Lua, _name: &str) -> Result<Option<Value>> {
    Ok(None)
}

pub(crate) fn class_member(
    _lua: &Lua,
    _ctx: &Ctx,
    _referent: Ref,
    _class: &str,
    _key: &str,
) -> Result<Option<Value>> {
    Ok(None)
}
