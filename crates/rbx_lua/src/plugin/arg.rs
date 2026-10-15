//! [`Arg`] ⇄ Luau values.

use mlua::{Lua, Result, Value};

use super::api::Arg;
use crate::ctx::Ctx;

/// `Arg::Instance` → instance userdata on `ctx`; `Enum` → the enum item
/// (resolved through the reflection database; an unknown item is an
/// error); `Table`/`List` → fresh tables; `Vector2`/`Vector3`/`Color3`/
/// `BrickColor` → their datatype userdata.
pub(crate) fn to_lua(lua: &Lua, ctx: &Ctx, arg: &Arg) -> Result<Value> {
    todo!()
}

/// The inverse, best effort: instances, datatypes, enum items, numbers,
/// strings, booleans; a table with only 1..n keys is a `List`, any other
/// table a `Table` (keys converted with `tostring`); functions, threads
/// and unknown userdata become `Nil`. Recursion stops at depth 16.
pub(crate) fn from_lua(value: &Value) -> Arg {
    todo!()
}

/// Several values at once, for `Signal:Fire` and friends.
pub(crate) fn to_lua_multi(lua: &Lua, ctx: &Ctx, args: &[Arg]) -> Result<mlua::MultiValue> {
    todo!()
}
