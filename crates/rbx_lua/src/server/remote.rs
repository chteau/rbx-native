//! Remotes between a server VM and its clients' VMs, all in one process:
//! `RemoteEvent`, `UnreliableRemoteEvent` and `RemoteFunction` calls leave
//! a VM as [`Outgoing`] packets, their arguments flattened to [`Wire`]
//! values on the way out, and the simulation hands each one to the VM it
//! is for with [`super::ServerVm::receive`].
//!
//! What survives the trip follows the creator-docs "Remote events and
//! callbacks" limits and the RemoteArgs golden: functions and metatables
//! are lost, instances the receiver doesn't have arrive as `nil`, and
//! tables are re-keyed the way Roblox re-keys them.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use mlua::{Function, IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::events::{instance_key, ref_key, registry_table, ON_INVOKE_KEY};
use crate::engine::{engine, scheduler, signal};
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;
use crate::property::from_lua::lua_to_variant;
use crate::property::to_lua::variant_to_lua;

/// Datatypes a remote carries as themselves.
const DATATYPES: &[&str] = &[
    "Vector3",
    "Vector2",
    "CFrame",
    "Color3",
    "UDim",
    "UDim2",
    "NumberRange",
    "BrickColor",
    "NumberSequence",
    "ColorSequence",
    "Rect",
    "PhysicalProperties",
    "Font",
];

/// Remote events waiting for a first listener before they are dropped
/// (Roblox's "invocation queue exhausted").
const QUEUE_LIMIT: usize = 256;

/// A value on its way between two VMs.
#[derive(Clone, Debug, PartialEq)]
pub enum Wire {
    Nil,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Buffer(Vec<u8>),
    Instance(Ref),
    Enum {
        enum_name: String,
        item: String,
        value: u32,
    },
    /// A datatype and its Roblox type name.
    Data(Variant, String),
    /// Keys and values in the order they are rebuilt.
    Table(Vec<(Wire, Wire)>),
}

impl Wire {
    /// How many bytes this value counts for against the 1000 byte
    /// `UnreliableRemoteEvent` limit, fitted to the RemotePayload golden
    /// (the largest payloads a real server let through): a type byte, a
    /// length as a varint, then the raw fields. 997 characters of string
    /// fit, 996 bytes of buffer, 110 numbers or 76 Vector3s in an array
    /// (whose keys don't travel), 142 four-letter keys set to `true`.
    /// Roblox compresses buffers, which this doesn't: a buffer of zeros
    /// counts its full length here (4000 of them still get through).
    pub fn size(&self) -> usize {
        match self {
            Wire::Nil => 1,
            Wire::Bool(_) => 2,
            Wire::Number(_) | Wire::Instance(_) => 9,
            Wire::String(bytes) => 1 + varint(bytes.len()) + bytes.len(),
            Wire::Buffer(bytes) => 2 + varint(bytes.len()) + bytes.len(),
            Wire::Enum { .. } => 5,
            Wire::Data(_, kind) => {
                1 + match kind.as_str() {
                    "BrickColor" => 4,
                    "Vector2" | "UDim" => 8,
                    "Vector3" | "Color3" => 12,
                    "CFrame" => 48,
                    _ => 16,
                }
            }
            Wire::Table(entries) => {
                let array = entries
                    .iter()
                    .enumerate()
                    .all(|(i, (k, _))| *k == Wire::Number((i + 1) as f64));
                let key = |k: &Wire| match k {
                    _ if array => 0,
                    // A dictionary's string keys go without a type byte.
                    Wire::String(bytes) => varint(bytes.len()) + bytes.len(),
                    other => other.size(),
                };
                3 + entries
                    .iter()
                    .map(|(k, v)| key(k) + v.size())
                    .sum::<usize>()
            }
        }
    }
}

/// Bytes a length takes as a varint (7 bits a byte).
fn varint(n: usize) -> usize {
    match n {
        0..=0x7F => 1,
        0x80..=0x3FFF => 2,
        _ => 3,
    }
}

/// The most an `UnreliableRemoteEvent` carries: "Events with payloads
/// larger than 1000 bytes are dropped" (creator-docs).
const UNRELIABLE_LIMIT: usize = 1000;

/// Who a packet is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    Server,
    Player(Ref),
    AllPlayers,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Event {
        remote: Ref,
        args: Vec<Wire>,
        unreliable: bool,
    },
    Invoke {
        remote: Ref,
        id: u32,
        args: Vec<Wire>,
    },
    Reply {
        id: u32,
        result: std::result::Result<Vec<Wire>, String>,
    },
    /// A chat message (`TextChannel:SendAsync`): to the server from a
    /// client, and from the server to each client it delivers to, with the
    /// sender's `TextSource`.
    Chat {
        channel: Ref,
        text: String,
        id: String,
        source: Option<Ref>,
    },
    /// A client's drag of a `DragDetector` that doesn't run locally, for
    /// the server to move and fire: the phase (`start`, `continue`, `end`),
    /// the part clicked, and the cursor ray and view frame as numbers.
    Drag {
        phase: String,
        detector: Ref,
        part: Option<Ref>,
        numbers: Vec<f64>,
    },
}

/// A packet a VM sent, with the sending DOM's mutation count at the time:
/// the replication stream delivers DOM changes and remotes in that order.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub to: To,
    pub packet: Packet,
    pub at: u64,
    /// The script line that sent it, while remotes are traced
    /// ([`super::ServerVm::trace_remotes`]).
    pub caller: Option<Caller>,
}

/// Where a remote call was made: the script's full name and the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub script: String,
    pub line: usize,
}

/// The innermost script frame of the running thread.
fn caller(lua: &Lua) -> Option<Caller> {
    let (script, line, _) = scheduler::script_frames(lua).into_iter().next()?;
    Some(Caller {
        script,
        line: line.unwrap_or(0),
    })
}

/// One VM's remote traffic: what it sent, and what arrived before anyone
/// could take it.
#[derive(Default)]
pub(super) struct Remotes {
    pub(super) outbox: Vec<Outgoing>,
    /// Events for a signal nobody listens to yet: (signal, sender, args).
    events: VecDeque<(signal::SignalKey, Option<Ref>, Vec<Wire>)>,
    /// Invocations of a remote with no callback yet.
    invokes: VecDeque<(Option<Ref>, Ref, u32, Vec<Wire>)>,
    /// Unreliable remotes already warned about losing an event for want
    /// of a listener.
    discarded: std::collections::HashSet<Ref>,
    /// Tracing: callers are noted and over-limit events kept in `oversized`.
    pub(super) trace: bool,
    pub(super) oversized: Vec<Outgoing>,
}

pub(super) type Shared = Rc<RefCell<Remotes>>;

fn is(ctx: &Ctx, class: &str, base: &str) -> bool {
    ctx.database().is_subclass_of(class, base)
}

fn error(text: &str) -> mlua::Error {
    mlua::Error::runtime(text.to_owned())
}

/// The remote methods for `referent`, `None` for any other member. `client`
/// says which side this VM is.
pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    remotes: &Shared,
    client: bool,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let event = is(ctx, class, "BaseRemoteEvent");
    let function = is(ctx, class, "RemoteFunction");
    if !event && !function {
        return Ok(None);
    }
    let unreliable = is(ctx, class, "UnreliableRemoteEvent");
    let ctx = ctx.clone();
    let remotes = remotes.clone();
    let send = move |lua: &Lua, ctx: &Ctx, to: To, packet: Packet| {
        let at = ctx.dom().mutations();
        let mut remotes = remotes.borrow_mut();
        let caller = if remotes.trace { caller(lua) } else { None };
        let out = Outgoing {
            to,
            packet,
            at,
            caller,
        };
        if let Packet::Event {
            args,
            unreliable: true,
            ..
        } = &out.packet
        {
            let size: usize = args.iter().map(Wire::size).sum();
            if size > UNRELIABLE_LIMIT {
                // Studio says how far over it went; the event is dropped.
                let name =
                    crate::instance::tree::full_name(&ctx.dom(), referent).unwrap_or_default();
                engine(lua).host.output(
                    crate::LineKind::Warn,
                    format!(
                        "UnreliableRemoteEvent {name} dropped an event: its payload is {} bytes over the {UNRELIABLE_LIMIT} byte limit",
                        size - UNRELIABLE_LIMIT
                    ),
                );
                if remotes.trace {
                    remotes.oversized.push(out);
                }
                return;
            }
        }
        remotes.outbox.push(out);
    };
    // Each side's half of the API (the RemoteArgs golden).
    match key {
        "OnServerEvent" if event && client => {
            return Err(error("OnServerEvent can only be used on the server"))
        }
        "OnClientEvent" if event && !client => {
            return Err(error("OnClientEvent can only be used on the client"))
        }
        "OnServerInvoke" | "OnClientInvoke" if function => {
            return Err(error(&format!(
                "{key} is a callback member of RemoteFunction; you can only set the callback value, get is not available"
            )))
        }
        _ => {}
    }
    let f = match key {
        "FireServer" if event => lua.create_function(move |lua, args: MultiValue| {
            if !client {
                return Err(error("FireServer can only be called from the client"));
            }
            let args = encode_args(lua, args.into_iter().skip(1))?;
            send(
                lua,
                &ctx,
                To::Server,
                Packet::Event {
                    remote: referent,
                    args,
                    unreliable,
                },
            );
            Ok(())
        })?,
        "FireClient" | "FireAllClients" if event => {
            let all = key == "FireAllClients";
            lua.create_function(move |lua, args: MultiValue| {
                if client {
                    return Err(error(if all {
                        "FireAllClients can only be called from the server"
                    } else {
                        "FireClient can only be called from the server"
                    }));
                }
                let mut rest = args.into_iter().skip(1);
                let to = if all {
                    To::AllPlayers
                } else {
                    To::Player(player_arg(&ctx, rest.next(), "FireClient")?)
                };
                let args = encode_args(lua, rest)?;
                send(
                    lua,
                    &ctx,
                    to,
                    Packet::Event {
                        remote: referent,
                        args,
                        unreliable,
                    },
                );
                Ok(())
            })?
        }
        "InvokeServer" | "InvokeClient" if function => {
            let server = key == "InvokeServer";
            let start = lua.create_function(move |lua, args: MultiValue| {
                // The wrong side's call never returns.
                if server != client {
                    return Ok(scheduler::new_await(lua));
                }
                let mut rest = args.into_iter().skip(1);
                let to = if server {
                    To::Server
                } else {
                    To::Player(player_arg(&ctx, rest.next(), "InvokeClient")?)
                };
                let args = encode_args(lua, rest)?;
                let id = scheduler::new_await(lua);
                send(
                    lua,
                    &ctx,
                    to,
                    Packet::Invoke {
                        remote: referent,
                        id,
                        args,
                    },
                );
                Ok(id)
            })?;
            scheduler::yielding(lua, start)?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}

/// The player a `…Client` call names: a `Player` instance, or the error
/// Roblox gives.
fn player_arg(ctx: &Ctx, value: Option<Value>, method: &str) -> Result<Ref> {
    if let Some(Value::UserData(data)) = &value {
        if let Ok(instance) = data.borrow::<LuaInstance>() {
            let referent = instance.referent();
            if ctx.dom().get(referent).map(|i| i.class()) == Some("Player") {
                return Ok(referent);
            }
        }
    }
    Err(mlua::Error::runtime(format!(
        "{method}: player argument must be a Player object"
    )))
}

fn encode_args(lua: &Lua, args: impl Iterator<Item = Value>) -> Result<Vec<Wire>> {
    args.map(|v| encode(lua, &v, &mut Vec::new())).collect()
}

/// `value` as it leaves the VM.
fn encode(lua: &Lua, value: &Value, path: &mut Vec<*const std::ffi::c_void>) -> Result<Wire> {
    Ok(match value {
        Value::Nil => Wire::Nil,
        Value::Boolean(b) => Wire::Bool(*b),
        Value::Integer(i) => Wire::Number(*i as f64),
        Value::Number(n) => Wire::Number(*n),
        Value::String(s) => Wire::String(s.as_bytes().to_vec()),
        Value::Buffer(b) => Wire::Buffer(b.to_vec()),
        Value::Table(table) => encode_table(lua, table, path)?,
        Value::UserData(data) => {
            if let Ok(instance) = data.borrow::<LuaInstance>() {
                return Ok(Wire::Instance(instance.referent()));
            }
            if let Ok(item) = data.borrow::<LuaEnumItem>() {
                return Ok(Wire::Enum {
                    enum_name: item.enum_name().to_owned(),
                    item: item.item_name().to_owned(),
                    value: item.value(),
                });
            }
            let kind: String = lua.globals().get::<Function>("typeof")?.call(value)?;
            if !DATATYPES.contains(&kind.as_str()) {
                return Ok(Wire::Nil);
            }
            let ctx = engine(lua).ctx.clone();
            match lua_to_variant(&ctx, "", &kind, value) {
                Ok(variant) => Wire::Data(variant, kind),
                Err(_) => Wire::Nil,
            }
        }
        _ => Wire::Nil,
    })
}

/// A table's entries as Roblox sends them (the RemoteArgs golden): with a
/// `[1]`, an array of its positive whole keys (holes stay holes, other keys
/// are dropped); without, a dictionary whose keys become strings (numbers
/// whole, instances and datatypes as `<Type> (value)`; a boolean key is an
/// error). Values that can't be sent (functions) are dropped, and so is
/// the metatable.
fn encode_table(lua: &Lua, table: &Table, path: &mut Vec<*const std::ffi::c_void>) -> Result<Wire> {
    let id = table.to_pointer();
    if path.contains(&id) {
        return Err(error("tables cannot be cyclic"));
    }
    path.push(id);
    let mut pairs = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (k, v) = pair?;
        pairs.push((k, v));
    }
    let index = |k: &Value| match k {
        Value::Integer(i) if *i >= 1 => Some(*i as u64),
        Value::Number(n) if n.fract() == 0.0 && *n >= 1.0 && *n < 2f64.powi(53) => Some(*n as u64),
        _ => None,
    };
    let array = pairs.iter().any(|(k, _)| index(k) == Some(1));
    let mut entries = Vec::with_capacity(pairs.len());
    if array {
        pairs.retain(|(k, _)| index(k).is_some());
        pairs.sort_by_key(|(k, _)| index(k));
        for (k, v) in &pairs {
            let value = encode(lua, v, path)?;
            if value != Wire::Nil {
                entries.push((Wire::Number(index(k).unwrap_or(1) as f64), value));
            }
        }
    } else {
        for (k, v) in &pairs {
            let key = match k {
                Value::String(s) => s.as_bytes().to_vec(),
                Value::Integer(i) => i.to_string().into_bytes(),
                Value::Number(n) => format!("{}", n.trunc() as i64).into_bytes(),
                Value::UserData(_) => {
                    let kind: String = lua.globals().get::<Function>("typeof")?.call(k)?;
                    let text: String = lua.globals().get::<Function>("tostring")?.call(k)?;
                    format!("<{kind}> ({text})").into_bytes()
                }
                _ => return Err(error("Invalid table key type used")),
            };
            let value = encode(lua, v, path)?;
            if value == Wire::Nil {
                continue;
            }
            entries.push((Wire::String(key), value));
        }
    }

    path.pop();
    Ok(Wire::Table(entries))
}

fn decode(lua: &Lua, ctx: &Ctx, wire: &Wire) -> Result<Value> {
    Ok(match wire {
        Wire::Nil => Value::Nil,
        Wire::Bool(b) => Value::Boolean(*b),
        Wire::Number(n) => Value::Number(*n),
        Wire::String(s) => Value::String(lua.create_string(s)?),
        Wire::Buffer(bytes) => Value::Buffer(lua.create_buffer(bytes)?),
        Wire::Instance(r) => {
            if ctx.dom().in_tree(*r) {
                LuaInstance::new(*r, ctx.clone()).into_lua(lua)?
            } else {
                Value::Nil
            }
        }
        Wire::Enum {
            enum_name,
            item,
            value,
        } => {
            LuaEnumItem::new(enum_name.clone(), item.clone(), *value, ctx.clone()).interned(lua)?
        }
        Wire::Data(variant, kind) => variant_to_lua(lua, ctx, "", kind, variant)?,
        Wire::Table(entries) => {
            let table = lua.create_table()?;
            for (k, v) in entries {
                let key = decode(lua, ctx, k)?;
                if key != Value::Nil {
                    table.raw_set(key, decode(lua, ctx, v)?)?;
                }
            }
            Value::Table(table)
        }
    })
}

fn decode_all(lua: &Lua, wires: &[Wire]) -> Result<MultiValue> {
    let ctx = engine(lua).ctx.clone();
    wires.iter().map(|w| decode(lua, &ctx, w)).collect()
}

/// Runs a remote callback in a thread of its own and sends back what it
/// returned or the error it raised; a callback that yields holds the
/// caller until it returns.
const DISPATCH: &str = r#"
local reply = ...
return function(callback, id, ...)
	reply(id, pcall(callback, ...))
end
"#;

fn dispatcher(lua: &Lua, remotes: &Shared, to: To) -> Result<Function> {
    let remotes = remotes.clone();
    let reply = lua.create_function(move |lua, args: MultiValue| {
        let mut args = args.into_iter();
        let id = match args.next() {
            Some(Value::Integer(i)) => i as u32,
            Some(Value::Number(n)) => n as u32,
            _ => return Ok(()),
        };
        let ok = matches!(args.next(), Some(Value::Boolean(true)));
        let result = if ok {
            Ok(encode_args(lua, args)?)
        } else {
            Err(match args.next() {
                Some(v) => v.to_string()?,
                None => "error".to_owned(),
            })
        };
        let at = engine(lua).ctx.dom().mutations();
        remotes.borrow_mut().outbox.push(Outgoing {
            to,
            packet: Packet::Reply { id, result },
            at,
            caller: None,
        });
        Ok(())
    })?;
    lua.load(DISPATCH).set_name("=RemoteFunction").call(reply)
}

/// A packet arriving at this VM from `from` (the sending player, `None`
/// when the server sent it).
pub(super) fn receive(
    lua: &Lua,
    remotes: &Shared,
    client: bool,
    from: Option<Ref>,
    packet: Packet,
) -> Result<()> {
    match packet {
        Packet::Event {
            remote,
            args,
            unreliable,
        } => {
            let name = if client {
                "OnClientEvent"
            } else {
                "OnServerEvent"
            };
            let key = instance_key(remote, name);
            // "Unlike a RemoteEvent, Roblox discards the message
            // immediately instead of queuing it" (creator-docs).
            if unreliable && !engine(lua).signals.borrow().has(&key) {
                if remotes.borrow_mut().discarded.insert(remote) {
                    let path = crate::instance::tree::full_name(&engine(lua).ctx.dom(), remote)
                        .unwrap_or_default();
                    engine(lua).host.output(
                        crate::LineKind::Warn,
                        format!("Remote event invocation discarded for {path}; did you forget to implement {name}?"),
                    );
                }
                return Ok(());
            }
            remotes.borrow_mut().events.push_back((key, from, args));
        }
        Packet::Invoke { remote, id, args } => {
            remotes
                .borrow_mut()
                .invokes
                .push_back((from, remote, id, args));
        }
        Packet::Reply { id, result } => {
            let result = match result {
                Ok(values) => Ok(decode_all(lua, &values)?),
                Err(message) => Err(message),
            };
            scheduler::resolve(lua, id, result)?;
        }
        // The chat and drag modules take these (`ServerVm::receive`).
        Packet::Chat { .. } | Packet::Drag { .. } => {}
    }
    flush(lua, remotes, client)
}

/// Hands queued events to their listeners and queued invocations to their
/// callbacks, in arrival order, keeping what still has nobody to take it.
pub(super) fn flush(lua: &Lua, remotes: &Shared, client: bool) -> Result<()> {
    let ctx = engine(lua).ctx.clone();
    let events = std::mem::take(&mut remotes.borrow_mut().events);
    let mut kept = VecDeque::new();
    for (key, from, args) in events {
        if !engine(lua).signals.borrow().has(&key) {
            kept.push_back((key, from, args));
            continue;
        }
        let mut values = decode_all(lua, &args)?;
        if let Some(player) = from.filter(|_| !client) {
            values.push_front(LuaInstance::new(player, ctx.clone()).into_lua(lua)?);
        }
        signal::fire_values(lua, &key, values)?;
    }
    while kept.len() > QUEUE_LIMIT {
        if let Some((key, ..)) = kept.pop_front() {
            let name = match key.owner {
                signal::Owner::Instance(r) => {
                    crate::instance::tree::full_name(&ctx.dom(), r).unwrap_or_default()
                }
                _ => String::new(),
            };
            engine(lua).host.output(
                crate::LineKind::Warn,
                format!(
                    "Remote event invocation queue exhausted for {name}; did you forget to implement {}?",
                    key.event
                ),
            );
        }
    }
    let mut remotes_mut = remotes.borrow_mut();
    remotes_mut.events.extend(kept);
    let invokes = std::mem::take(&mut remotes_mut.invokes);
    drop(remotes_mut);
    let side = if client { "client" } else { "server" };
    let mut waiting = VecDeque::new();
    for (from, remote, id, args) in invokes {
        let callback: Option<Function> =
            registry_table(lua, ON_INVOKE_KEY)?.get(format!("{side}:{}", ref_key(remote)))?;
        let Some(callback) = callback else {
            waiting.push_back((from, remote, id, args));
            continue;
        };
        let to = match from {
            Some(player) => To::Player(player),
            None => To::Server,
        };
        let mut values = decode_all(lua, &args)?;
        if let Some(player) = from.filter(|_| !client) {
            values.push_front(LuaInstance::new(player, ctx.clone()).into_lua(lua)?);
        }
        values.push_front(Value::Integer(i64::from(id)));
        values.push_front(Value::Function(callback));
        scheduler::spawn(lua, dispatcher(lua, remotes, to)?, values)?;
    }
    remotes.borrow_mut().invokes.extend(waiting);
    Ok(())
}
