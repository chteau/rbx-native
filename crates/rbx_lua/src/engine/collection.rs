//! `CollectionService`, over the `Tags` property `Instance:AddTag` writes
//! too (see `instance::tags`).

use mlua::{Lua, Result, Table, Value};
use rbx_dom::Ref;

use super::engine;
use super::signal::{self, LuaSignal, Owner, SignalKey};
use crate::instance::{tags, LuaInstance};
use crate::plugin::api::Arg;

/// `CollectionService[key]`: `AddTag(instance, tag)`, `RemoveTag`,
/// `HasTag`, `GetTags`, `GetTagged(tag)` (the place's instances only, in
/// tree order) and `GetInstanceAddedSignal(tag)` /
/// `GetInstanceRemovedSignal(tag)`, which fire for tags changed through
/// this service; `None` for any other key.
pub(crate) fn member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "GetTagged" => lua.create_function(|lua, (_, tag): (Value, String)| {
            let engine = engine(lua);
            let found = tagged(&engine, &tag);
            instances_table(lua, &found)
        })?,
        "HasTag" => lua.create_function(|lua, (_, inst, tag): (Value, Value, String)| {
            Ok(tags::get(&engine(lua).ctx, instance_ref(&inst)?).contains(&tag))
        })?,
        "GetTags" => lua.create_function(|lua, (_, inst): (Value, Value)| {
            lua.create_sequence_from(tags::get(&engine(lua).ctx, instance_ref(&inst)?))
        })?,
        "AddTag" | "RemoveTag" => {
            let add = key == "AddTag";
            lua.create_function(move |lua, (_, inst, tag): (Value, Value, String)| {
                let referent = instance_ref(&inst)?;
                if tags::set(&engine(lua).ctx, referent, &tag, add)? {
                    let event = if add {
                        "InstanceAdded"
                    } else {
                        "InstanceRemoved"
                    };
                    signal::fire(lua, &tag_key(event, &tag), &[Arg::Instance(referent)])?;
                }
                Ok(())
            })?
        }
        "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" => {
            let event = key[3..key.len() - 6].to_string();
            lua.create_function(move |_, (_, tag): (Value, String)| {
                Ok(LuaSignal(tag_key(&event, &tag)))
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

fn tag_key(event: &str, tag: &str) -> SignalKey {
    SignalKey::new(
        Owner::Service("CollectionService"),
        format!("{event}:{tag}"),
    )
}

fn tagged(engine: &super::Engine, tag: &str) -> Vec<Ref> {
    let dom = engine.ctx.dom();
    let mut stack: Vec<Ref> = dom
        .root_refs()
        .iter()
        .rev()
        .filter(|r| !engine.host.is_parked(**r))
        .copied()
        .collect();
    let mut found = Vec::new();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        if rbx_dom::attributes::tags(instance.properties().get("Tags")).contains(&tag) {
            found.push(r);
        }
        stack.extend(instance.children().iter().rev());
    }
    found
}

fn instance_ref(value: &Value) -> Result<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
        .ok_or_else(|| mlua::Error::runtime("Instance expected"))
}

fn instances_table(lua: &Lua, refs: &[Ref]) -> Result<Table> {
    let ctx = engine(lua).ctx.clone();
    let table = lua.create_table()?;
    for (i, r) in refs.iter().enumerate() {
        table.raw_set(i + 1, LuaInstance::new(*r, ctx.clone()))?;
    }
    Ok(table)
}
