//! `CollectionService`, over the `Tags` property `Instance:AddTag` writes
//! too (see `instance::tags`).
//!
//! What a live server does (the CollectionTags golden): only instances in
//! the DataModel count. `GetInstanceAddedSignal(tag)` fires, before the
//! line after, when a tagged instance comes into the DataModel or an
//! instance in it gets the tag; `GetInstanceRemovedSignal(tag)` when it
//! leaves (Destroy included) or loses the tag. `TagAdded`/`TagRemoved`
//! fire deferred when the first instance with a tag comes in and the last
//! one goes. Each VM keeps the tags it has seen on the DataModel's
//! instances, so tags written any way (a replicated `Tags`, a clone
//! parented in) fire once.

use std::collections::HashMap;

use mlua::{Lua, MultiValue, Result, Table, Value};
use rbx_dom::Ref;

use super::signal::{self, LuaSignal, Owner, SignalKey};
use super::{engine, Engine};
use crate::instance::{tags, LuaInstance};
use crate::plugin::api::Arg;

/// The tags this VM has seen on instances in the DataModel.
#[derive(Default)]
struct Seen {
    ready: bool,
    known: HashMap<Ref, Vec<String>>,
    counts: HashMap<String, usize>,
}

/// `CollectionService[key]`, `None` for any other key.
pub(crate) fn member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    let f = match key {
        "GetTagged" => lua.create_function(|lua, (_, tag): (Value, Value)| {
            let tag = tag_arg(&tag, 1)?;
            let engine = engine(lua);
            let found = tagged(&engine, &tag);
            instances_table(lua, &found)
        })?,
        "GetAllTags" => lua.create_function(|lua, _: MultiValue| {
            ready(lua);
            let mut all: Vec<String> = seen(lua, |s| {
                s.counts
                    .iter()
                    .filter(|(_, n)| **n > 0)
                    .map(|(t, _)| t.clone())
                    .collect()
            });
            all.sort();
            lua.create_sequence_from(all)
        })?,
        "HasTag" => lua.create_function(|lua, (_, inst, tag): (Value, Value, Value)| {
            let Ok(referent) = instance_ref(&inst) else {
                return Ok(false);
            };
            Ok(tags::get(&engine(lua).ctx, referent).contains(&tag_arg(&tag, 3)?))
        })?,
        "GetTags" => lua.create_function(|lua, (_, inst): (Value, Value)| {
            lua.create_sequence_from(tags::get(&engine(lua).ctx, instance_ref(&inst)?))
        })?,
        "AddTag" | "RemoveTag" => {
            let add = key == "AddTag";
            lua.create_function(move |lua, (_, inst, tag): (Value, Value, Value)| {
                let referent = instance_ref(&inst)?;
                let tag = tag_arg(&tag, 3)?;
                if tags::set(&engine(lua).ctx, referent, &tag, add)? {
                    changed(lua, &[referent])?;
                }
                Ok(())
            })?
        }
        "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" => {
            let event = key[3..key.len() - 6].to_string();
            lua.create_function(move |_, (_, tag): (Value, Value)| {
                Ok(LuaSignal(tag_key(&event, &tag_arg(&tag, 1)?)))
            })?
        }
        "TagAdded" | "TagRemoved" => {
            return lua
                .create_userdata(LuaSignal(service_key(key)))
                .map(|u| Some(Value::UserData(u)));
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// A tag argument: a string, or a number as its text.
fn tag_arg(value: &Value, index: usize) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy().to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(super::json::number(*n)),
        Value::Nil => Err(mlua::Error::runtime(format!(
            "Argument {index} missing or nil"
        ))),
        other => Err(mlua::Error::runtime(format!(
            "Unable to cast {} to string",
            other.type_name()
        ))),
    }
}

fn tag_key(event: &str, tag: &str) -> SignalKey {
    SignalKey::new(
        Owner::Service("CollectionService"),
        format!("{event}:{tag}"),
    )
}

fn service_key(event: &str) -> SignalKey {
    SignalKey::new(Owner::Service("CollectionService"), event)
}

fn seen<R>(lua: &Lua, f: impl FnOnce(&mut Seen) -> R) -> R {
    if lua.app_data_ref::<std::cell::RefCell<Seen>>().is_none() {
        lua.set_app_data(std::cell::RefCell::new(Seen::default()));
    }
    let cell = lua
        .app_data_ref::<std::cell::RefCell<Seen>>()
        .expect("just set");
    let mut seen = cell.borrow_mut();
    f(&mut seen)
}

/// Whether `referent` is in the DataModel: under one of the place's
/// services, not the host's parked instances.
fn in_game(engine: &Engine, referent: Ref) -> bool {
    let dom = engine.ctx.dom();
    let mut at = referent;
    while let Some(parent) = dom.parent(at) {
        if dom.get(parent).is_none() {
            return false;
        }
        at = parent;
    }
    dom.get(at)
        .is_some_and(|i| engine.ctx.database().is_service(i.class()))
        && !engine.host.is_parked(at)
}

/// What the DataModel's instances are tagged with now, recorded without
/// firing anything, the first time it's needed.
fn ready(lua: &Lua) {
    if seen(lua, |s| s.ready) {
        return;
    }
    let engine = engine(lua);
    let mut known = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    {
        let dom = engine.ctx.dom();
        let mut stack: Vec<Ref> = place_roots(&engine);
        while let Some(r) = stack.pop() {
            let Some(instance) = dom.get(r) else { continue };
            let list: Vec<String> = rbx_dom::attributes::tags(instance.properties().get("Tags"))
                .into_iter()
                .map(str::to_owned)
                .collect();
            if !list.is_empty() {
                for t in &list {
                    *counts.entry(t.clone()).or_default() += 1;
                }
                known.insert(r, list);
            }
            stack.extend(instance.children().iter().copied());
        }
    }
    seen(lua, |s| {
        s.ready = true;
        s.known = known;
        s.counts = counts;
    });
}

/// `roots` and everything under them changed tags or came in or out of
/// the DataModel: fires what that calls for.
pub(crate) fn changed(lua: &Lua, roots: &[Ref]) -> Result<()> {
    let engine = engine(lua);
    let first = !seen(lua, |s| s.ready);
    if first {
        ready(lua);
        // `ready` recorded these as they are now; they changed just now,
        // so start them from nothing.
        seen(lua, |s| {
            for r in subtrees(&engine, roots) {
                if let Some(list) = s.known.remove(&r) {
                    for t in list {
                        if let Some(n) = s.counts.get_mut(&t) {
                            *n -= 1;
                        }
                    }
                }
            }
        });
    }
    let mut fire = Vec::new();
    for r in subtrees(&engine, roots) {
        let now = if in_game(&engine, r) {
            tags::get(&engine.ctx, r)
        } else {
            Vec::new()
        };
        seen(lua, |s| {
            let was = s.known.remove(&r).unwrap_or_default();
            for t in was.iter().filter(|t| !now.contains(t)) {
                let n = s.counts.entry(t.clone()).or_default();
                *n = n.saturating_sub(1);
                fire.push((r, t.clone(), false, *n == 0));
            }
            for t in now.iter().filter(|t| !was.contains(t)) {
                let n = s.counts.entry(t.clone()).or_default();
                *n += 1;
                fire.push((r, t.clone(), true, *n == 1));
            }
            if !now.is_empty() {
                s.known.insert(r, now);
            }
        });
    }
    for (r, tag, added, edge) in fire {
        let event = if added {
            "InstanceAdded"
        } else {
            "InstanceRemoved"
        };
        signal::fire(lua, &tag_key(event, &tag), &[Arg::Instance(r)])?;
        if edge {
            let name = if added { "TagAdded" } else { "TagRemoved" };
            signal::fire_deferred(lua, &service_key(name), &[Arg::String(tag)])?;
        }
    }
    Ok(())
}

/// The roots and their descendants; a destroyed root still counts (it
/// leaves the DataModel with everything it held).
fn subtrees(engine: &Engine, roots: &[Ref]) -> Vec<Ref> {
    let dom = engine.ctx.dom();
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = roots.to_vec();
    while let Some(r) = stack.pop() {
        out.push(r);
        if let Some(instance) = dom.get(r) {
            stack.extend(instance.children().iter().rev().copied());
        }
    }
    out
}

fn place_roots(engine: &Engine) -> Vec<Ref> {
    let dom = engine.ctx.dom();
    dom.root_refs()
        .iter()
        .rev()
        .filter(|r| {
            !engine.host.is_parked(**r)
                && dom
                    .get(**r)
                    .is_some_and(|i| engine.ctx.database().is_service(i.class()))
        })
        .copied()
        .collect()
}

fn tagged(engine: &Engine, tag: &str) -> Vec<Ref> {
    let dom = engine.ctx.dom();
    let mut stack = place_roots(engine);
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
