//! The members every instance has in both VMs: its signals, attributes,
//! `BindableEvent`/`BindableFunction`. What fires the instance signals is
//! `changes`.

use std::collections::BTreeMap;

use mlua::{Function, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Color3Data, Ref, Variant, Vector2Data, Vector3Data};
use rbx_reflection::ReflectionDatabase;

use super::signal::{self, LuaSignal, Owner, SignalKey};
use crate::ctx::Ctx;
use crate::plugin::api::Arg;
use crate::plugin::arg;

/// Where the place files keep the attribute blob.
pub(super) const ATTRIBUTES: &str = "AttributesSerialize";
/// The binary format's string type id, which the attribute blob travels as.
const STRING_TYPE_ID: u8 = 0x01;
const ON_INVOKE_KEY: &str = "__rbxn_on_invoke";

const INSTANCE_SIGNALS: &[&str] = &[
    "Changed",
    "ChildAdded",
    "ChildRemoved",
    "DescendantAdded",
    "DescendantRemoving",
    "AncestryChanged",
    "Destroying",
    "AttributeChanged",
];

pub(crate) fn instance_key(referent: Ref, event: impl Into<String>) -> SignalKey {
    SignalKey::new(Owner::Instance(referent), event)
}

/// Whether `key` names a signal every VM gives instances of `class`.
pub(crate) fn is_signal(db: &ReflectionDatabase, class: &str, key: &str) -> bool {
    let has = |list: &[&str], base: &str| list.contains(&key) && db.is_subclass_of(class, base);
    INSTANCE_SIGNALS.contains(&key)
        || has(&["Event"], "BindableEvent")
        || has(&["OnServerEvent", "OnClientEvent"], "RemoteEvent")
}

/// `instance[key]` for the members above: `None` lets the property and
/// child lookup answer.
///
/// - every instance: the `INSTANCE_SIGNALS`, `GetPropertyChangedSignal(
///   name)`, `GetAttributeChangedSignal(name)`, `GetAttribute`,
///   `SetAttribute`, `GetAttributes` (over the attribute blob, via
///   `rbx_dom::attributes`);
/// - `BindableEvent`: `Event`, `Fire(...)`; `BindableFunction`:
///   `Invoke(...)` (calls `OnInvoke` synchronously);
/// - `RemoteEvent` / `RemoteFunction`: with no network to cross, a remote
///   is a bindable with a player argument (`nil` for the server's view).
pub(crate) fn index(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let db = ctx.database();
    if is_signal(db, class, key) {
        return lua
            .create_userdata(LuaSignal(instance_key(referent, key)))
            .map(|u| Some(Value::UserData(u)));
    }
    if let Some(value) = attribute_function(lua, ctx, referent, key)? {
        return Ok(Some(value));
    }
    bindable_function(lua, db, referent, class, key)
}

/// `instance[key] = value` for the callbacks Roblox keeps outside
/// properties (`OnInvoke`, `OnServerInvoke`, `OnClientInvoke`): `true`
/// when handled.
pub(crate) fn new_index(
    lua: &Lua,
    ctx: &Ctx,
    referent: Ref,
    class: &str,
    key: &str,
    value: &Value,
) -> Result<bool> {
    let db = ctx.database();
    let slot = match key {
        "OnInvoke" if db.is_subclass_of(class, "BindableFunction") => ref_key(referent),
        "OnServerInvoke" if db.is_subclass_of(class, "RemoteFunction") => {
            format!("server:{}", ref_key(referent))
        }
        "OnClientInvoke" if db.is_subclass_of(class, "RemoteFunction") => {
            format!("client:{}", ref_key(referent))
        }
        _ => return Ok(false),
    };
    let callback = match value {
        Value::Function(f) => Some(f.clone()),
        Value::Nil => None,
        other => {
            return Err(mlua::Error::runtime(format!(
                "{key} must be a function, got {}",
                other.type_name()
            )))
        }
    };
    registry_table(lua, ON_INVOKE_KEY)?.set(slot, callback)?;
    Ok(true)
}

/// `game`'s own signals, keyed under `Owner::Service("DataModel")`;
/// `DescendantAdded`/`DescendantRemoving` fire for the place's instances
/// (see `changes`), the others are there to connect to.
pub(crate) fn game_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    Ok(Some(match key {
        "Changed" | "ChildAdded" | "ChildRemoved" | "DescendantAdded" | "DescendantRemoving"
        | "ItemChanged" | "Close" | "AncestryChanged" => {
            Value::UserData(lua.create_userdata(game_signal(key))?)
        }
        "GetPropertyChangedSignal" => {
            Value::Function(lua.create_function(|_, (_this, name): (Value, String)| {
                Ok(game_signal(format!("Changed:{name}")))
            })?)
        }
        _ => return Ok(None),
    }))
}

pub(super) fn game_signal(event: impl Into<String>) -> LuaSignal {
    LuaSignal(SignalKey::new(Owner::Service("DataModel"), event))
}

pub(crate) fn registry_table(lua: &Lua, name: &str) -> Result<Table> {
    if let Some(table) = lua.named_registry_value::<Option<Table>>(name)? {
        return Ok(table);
    }
    let table = lua.create_table()?;
    lua.set_named_registry_value(name, table.clone())?;
    Ok(table)
}

pub(crate) fn ref_key(referent: Ref) -> String {
    format!("{referent:?}")
}

pub(super) fn read_attributes(ctx: &Ctx, referent: Ref) -> BTreeMap<String, Variant> {
    let dom = ctx.dom();
    let blob = dom
        .get(referent)
        .and_then(|i| i.properties().get(ATTRIBUTES));
    rbx_dom::attributes::decode(blob)
}

fn variant_to_arg(variant: &Variant) -> Arg {
    match variant {
        Variant::String(s) => Arg::String(s.clone()),
        Variant::Bool(b) => Arg::Bool(*b),
        Variant::Int32(n) => Arg::Number(f64::from(*n)),
        Variant::Int64(n) => Arg::Number(*n as f64),
        Variant::Float32(n) => Arg::Number(f64::from(*n)),
        Variant::Float64(n) => Arg::Number(*n),
        Variant::Vector2(v) => Arg::Vector2([v.x, v.y]),
        Variant::Vector3(v) => Arg::Vector3([v.x, v.y, v.z]),
        Variant::Color3(c) => Arg::Color3([c.r, c.g, c.b]),
        Variant::BrickColor(n) => Arg::BrickColor(*n),
        _ => Arg::Nil,
    }
}

fn arg_to_variant(name: &str, value: Arg) -> Result<Option<Variant>> {
    Ok(Some(match value {
        Arg::Nil => return Ok(None),
        Arg::String(s) => Variant::String(s),
        Arg::Bool(b) => Variant::Bool(b),
        Arg::Number(n) => Variant::Float64(n),
        Arg::Vector2([x, y]) => Variant::Vector2(Vector2Data { x, y }),
        Arg::Vector3([x, y, z]) => Variant::Vector3(Vector3Data { x, y, z }),
        Arg::Color3([r, g, b]) => Variant::Color3(Color3Data { r, g, b }),
        Arg::BrickColor(n) => Variant::BrickColor(n),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "attribute '{name}' cannot hold a value of this type"
            )))
        }
    }))
}

fn set_attribute(ctx: &Ctx, referent: Ref, name: &str, value: Option<Variant>) -> Result<()> {
    ctx.check_write(referent)?;
    let mut attributes = read_attributes(ctx, referent);
    match value {
        Some(value) => attributes.insert(name.to_string(), value),
        None => attributes.remove(name),
    };
    let raw = rbx_dom::attributes::encode(&attributes, |_| None)
        .ok_or_else(|| mlua::Error::runtime("these attributes cannot be written back"))?;
    ctx.dom_mut()
        .set_property(
            referent,
            ATTRIBUTES,
            Variant::Unknown {
                type_id: STRING_TYPE_ID,
                raw,
            },
        )
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
    Ok(())
}

fn signal_getter(lua: &Lua, referent: Ref, prefix: &'static str) -> Result<Value> {
    lua.create_function(move |_, (_this, name): (Value, String)| {
        Ok(LuaSignal(instance_key(
            referent,
            format!("{prefix}:{name}"),
        )))
    })
    .map(Value::Function)
}

fn attribute_function(lua: &Lua, ctx: &Ctx, referent: Ref, key: &str) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let f = match key {
        "GetPropertyChangedSignal" => return signal_getter(lua, referent, "Changed").map(Some),
        "GetAttributeChangedSignal" => {
            return signal_getter(lua, referent, "AttributeChanged").map(Some)
        }
        "GetAttribute" => lua.create_function(move |lua, (_this, name): (Value, String)| {
            let value = read_attributes(&ctx, referent)
                .get(&name)
                .map(variant_to_arg);
            arg::to_lua(lua, &ctx, &value.unwrap_or(Arg::Nil))
        })?,
        "SetAttribute" => {
            lua.create_function(move |_, (_this, name, value): (Value, String, Value)| {
                let variant = arg_to_variant(&name, arg::from_lua(&value))?;
                set_attribute(&ctx, referent, &name, variant)
            })?
        }
        "GetAttributes" => lua.create_function(move |lua, _this: Value| {
            let table = lua.create_table()?;
            for (name, value) in read_attributes(&ctx, referent) {
                table.set(name, arg::to_lua(lua, &ctx, &variant_to_arg(&value))?)?;
            }
            Ok(table)
        })?,
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

fn bindable_function(
    lua: &Lua,
    db: &ReflectionDatabase,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let is = |base: &str| db.is_subclass_of(class, base);
    let f = match key {
        "Fire" if is("BindableEvent") => {
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                signal::fire_values(lua, &instance_key(referent, "Event"), args)
            })?
        }
        "Invoke" if is("BindableFunction") => {
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let callback: Option<Function> =
                    registry_table(lua, ON_INVOKE_KEY)?.get(ref_key(referent))?;
                match callback {
                    Some(callback) => callback.call::<MultiValue>(args),
                    None => Err(mlua::Error::runtime(
                        "OnInvoke is not set on this BindableFunction",
                    )),
                }
            })?
        }
        // `…Server` calls pass `nil` for the player the server would see;
        // `…Client` calls drop the player they name.
        "FireServer" if is("RemoteEvent") => {
            lua.create_function(move |lua, (_this, mut args): (Value, MultiValue)| {
                args.push_front(Value::Nil);
                signal::fire_values(lua, &instance_key(referent, "OnServerEvent"), args)
            })?
        }
        "FireClient" | "FireAllClients" if is("RemoteEvent") => {
            let skip = usize::from(key == "FireClient");
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let args: MultiValue = args.into_iter().skip(skip).collect();
                signal::fire_values(lua, &instance_key(referent, "OnClientEvent"), args)
            })?
        }
        "InvokeServer" | "InvokeClient" if is("RemoteFunction") => {
            let server = key == "InvokeServer";
            lua.create_function(move |lua, (_this, args): (Value, MultiValue)| {
                let side = if server { "server" } else { "client" };
                let callback: Option<Function> = registry_table(lua, ON_INVOKE_KEY)?
                    .get(format!("{side}:{}", ref_key(referent)))?;
                let mut args: MultiValue = args.into_iter().skip(usize::from(!server)).collect();
                if server {
                    args.push_front(Value::Nil);
                }
                match callback {
                    Some(callback) => callback.call::<MultiValue>(args),
                    None => Err(mlua::Error::runtime(format!(
                        "On{}Invoke is not set on this RemoteFunction",
                        if server { "Server" } else { "Client" }
                    ))),
                }
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
