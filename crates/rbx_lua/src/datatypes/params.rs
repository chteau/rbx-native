//! Query parameter objects (`RaycastParams`, `OverlapParams`), `RaycastResult`
//! and the small spatial value types `Region3`, `Faces` and `Axes`.

use mlua::{Lua, Result};

mod filter;
mod region;

#[allow(unused_imports)]
pub(crate) use filter::{LuaOverlapParams, LuaRaycastParams, LuaRaycastResult};

pub(crate) fn register(lua: &Lua) -> Result<()> {
    filter::register(lua)?;
    region::register(lua)
}
