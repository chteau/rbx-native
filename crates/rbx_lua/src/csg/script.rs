//! `BasePart:UnionAsync`, `SubtractAsync` and `IntersectAsync` in edit
//! scripts (the command bar, MCP's `execute_luau`): the result is a new
//! operation, unparented, the parts left as they were, as in Studio. A
//! runtime with its own hooks (Local Run) answers these first.

use mlua::{IntoLua, Lua, Result, Value};
use rbx_dom::Ref;

use super::{build, combine, Flavor, Operation};
use crate::ctx::Ctx;
use crate::instance::LuaInstance;

const UNKNOWN_PRIMITIVE: &str = "CSG: One of the primitives was of unknown type (error -4)";

fn referent(value: &Value) -> Option<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
}

/// The method `key` names on a `class` part, if it is one of the three.
pub(crate) fn member(lua: &Lua, ctx: &Ctx, class: &str, key: &str) -> Result<Option<Value>> {
    let operation = match key {
        "UnionAsync" => Operation::Union,
        "SubtractAsync" => Operation::Subtract,
        "IntersectAsync" => Operation::Intersect,
        _ => return Ok(None),
    };
    if !ctx.database().is_subclass_of(class, "BasePart") || class == "Terrain" {
        return Ok(None);
    }
    let ctx = ctx.clone();
    let f = lua.create_function(move |lua, (this, parts): (Value, Value)| {
        let base = referent(&this)
            .ok_or_else(|| mlua::Error::runtime("Expected ':' not '.' calling member function"))?;
        let Value::Table(list) = parts else {
            return Err(mlua::Error::runtime("Unable to cast value to Objects"));
        };
        let others = list
            .sequence_values::<Value>()
            .map(|v| {
                referent(&v?).ok_or_else(|| mlua::Error::runtime("Unable to cast value to Objects"))
            })
            .collect::<Result<Vec<Ref>>>()?;
        let db = ctx.database();
        let solid = {
            let dom = ctx.dom();
            let all_parts = std::iter::once(&base).chain(&others).all(|&r| {
                dom.get(r).is_some_and(|i| {
                    db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain"
                })
            });
            if !all_parts {
                return Err(mlua::Error::runtime(UNKNOWN_PRIMITIVE));
            }
            match combine(&dom, db, operation, base, &others) {
                Ok(solid) => solid,
                Err(why) if why == "Empty" => return Ok(Value::Nil),
                Err(why) => return Err(mlua::Error::runtime(format!("CSG: {why}"))),
            }
        };
        let made = build(
            &mut ctx.dom_mut(),
            db,
            operation,
            Flavor::Part,
            base,
            &others,
            solid,
            false,
        );
        match made.first() {
            Some(m) => LuaInstance::new(m.referent, ctx.clone()).into_lua(lua),
            None => Ok(Value::Nil),
        }
    })?;
    Ok(Some(Value::Function(f)))
}
