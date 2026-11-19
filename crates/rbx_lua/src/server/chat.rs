//! `TextChatService` in Local Run: our own `chat.luau` keeps the callbacks
//! and runs a message through the channels; messages travel between the
//! VMs as [`Packet::Chat`], and what a client received is kept for the
//! editor's chat window and bubbles ([`ChatLine`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mlua::{AnyUserData, Function, IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use super::remote::{Outgoing, Packet, Shared, To};
use crate::engine::engine;
use crate::engine::events::instance_key;
use crate::engine::signal::{self, LuaSignal};
use crate::instance::LuaInstance;

const SOURCE: &str = include_str!("chat.luau");
const KEY: &str = "rbxn.server.chat";

/// A message a client showed: who (the prefix, `Name:`; empty for a system
/// message), what, in which channel, and the sender's user id (0 for none).
#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    pub prefix: String,
    pub text: String,
    pub channel: String,
    pub user_id: i64,
}

/// What the chat module needs of its VM: the remote outbox, which side
/// it is, and where shown messages go.
#[derive(Clone)]
pub(super) struct Chat {
    pub(super) remotes: Shared,
    pub(super) client: Rc<Cell<bool>>,
    pub(super) log: Rc<RefCell<Vec<ChatLine>>>,
}

fn referent(value: &AnyUserData) -> Result<Ref> {
    Ok(value.borrow::<LuaInstance>()?.referent())
}

fn instance(lua: &Lua, r: Option<Ref>) -> Result<Value> {
    match r {
        Some(r) => LuaInstance::new(r, engine(lua).ctx.clone()).into_lua(lua),
        None => Ok(Value::Nil),
    }
}

fn host(lua: &Lua, chat: &Chat) -> Result<Table> {
    let host = lua.create_table()?;
    let client = chat.client.clone();
    host.set(
        "client",
        lua.create_function(move |_, ()| Ok(client.get()))?,
    )?;
    host.set(
        "id",
        lua.create_function(|_, target: AnyUserData| Ok(referent(&target)?.value()))?,
    )?;
    host.set(
        "guid",
        lua.create_function(|_, ()| Ok(crate::engine::json::guid(false)))?,
    )?;
    host.set(
        "new",
        lua.create_function(|lua, class: String| {
            let r = engine(lua).ctx.dom_mut().new_instance(&class, &class, None);
            instance(lua, Some(r))
        })?,
    )?;
    // A property write past the scripts' read-only rule (a message's
    // `Status`, a source's `UserId`).
    host.set(
        "write",
        lua.create_function(|lua, (target, name, value): (AnyUserData, String, Value)| {
            let ctx = engine(lua).ctx.clone();
            let r = referent(&target)?;
            let class = ctx.dom().get(r).map(|i| i.class().to_owned());
            let Some(kind) = class.and_then(|c| {
                ctx.database()
                    .resolve_property(&c, &name)
                    .map(|p| p.value_type.clone())
            }) else {
                return Ok(());
            };
            if value.is_nil() {
                let _ = ctx.dom_mut().remove_property(r, &name);
                return Ok(());
            }
            let variant: Variant =
                crate::property::from_lua::lua_to_variant(&ctx, &name, &kind, &value)?;
            let _ = ctx.dom_mut().set_property(r, &name, variant);
            Ok(())
        })?,
    )?;
    host.set(
        "fire",
        lua.create_function(
            |lua, (owner, name, args): (AnyUserData, String, MultiValue)| {
                signal::fire_values(lua, &instance_key(referent(&owner)?, name), args)
            },
        )?,
    )?;
    let outbox = chat.remotes.clone();
    host.set(
        "send",
        lua.create_function(
            move |lua, (channel, text, id): (AnyUserData, String, String)| {
                push(
                    lua,
                    &outbox,
                    To::Server,
                    referent(&channel)?,
                    text,
                    id,
                    None,
                );
                Ok(())
            },
        )?,
    )?;
    let outbox = chat.remotes.clone();
    host.set(
        "deliver",
        lua.create_function(
            move |lua,
                  (player, channel, text, id, source): (
                AnyUserData,
                AnyUserData,
                String,
                String,
                Option<AnyUserData>,
            )| {
                let source = source.map(|s| referent(&s)).transpose()?;
                let to = To::Player(referent(&player)?);
                push(lua, &outbox, to, referent(&channel)?, text, id, source);
                Ok(())
            },
        )?,
    )?;
    let log = chat.log.clone();
    host.set(
        "log",
        lua.create_function(
            move |_, (prefix, text, channel, user_id): (String, String, String, i64)| {
                log.borrow_mut().push(ChatLine {
                    prefix,
                    text,
                    channel,
                    user_id,
                });
                Ok(())
            },
        )?,
    )?;
    Ok(host)
}

fn push(
    lua: &Lua,
    remotes: &Shared,
    to: To,
    channel: Ref,
    text: String,
    id: String,
    source: Option<Ref>,
) {
    let at = engine(lua).ctx.dom().mutations();
    remotes.borrow_mut().outbox.push(Outgoing {
        to,
        packet: Packet::Chat {
            channel,
            text,
            id,
            source,
        },
        at,
    });
}

/// The module, made the first time it's needed with this VM's side.
fn module(lua: &Lua, chat: &Chat) -> Result<Table> {
    if let Some(module) = lua.named_registry_value::<Option<Table>>(KEY)? {
        return Ok(module);
    }
    let module: Table = lua
        .load(SOURCE)
        .set_name("=TextChatService")
        .call(host(lua, chat)?)?;
    lua.set_named_registry_value(KEY, module.clone())?;
    Ok(module)
}

const SIGNALS: &[(&str, &str)] = &[
    ("TextChatService", "MessageReceived"),
    ("TextChatService", "SendingMessage"),
    ("TextChatService", "BubbleDisplayed"),
    ("TextChannel", "MessageReceived"),
];

/// A member of `TextChatService` or a `TextChannel`.
pub(super) fn member(
    lua: &Lua,
    chat: &Chat,
    owner: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    if SIGNALS.contains(&(class, key)) {
        let signal = LuaSignal(instance_key(owner, key));
        return lua
            .create_userdata(signal)
            .map(|u| Some(Value::UserData(u)));
    }
    let (value, found): (Value, bool) = module(lua, chat)?.get::<Function>("member")?.call((
        class,
        instance(lua, Some(owner))?,
        key,
    ))?;
    Ok(found.then_some(value))
}

/// A callback written (`TextChatService.OnIncomingMessage = f`); whether
/// it was one.
pub(super) fn set(
    lua: &Lua,
    chat: &Chat,
    owner: Ref,
    class: &str,
    key: &str,
    value: &Value,
) -> Result<bool> {
    if !matches!(class, "TextChatService" | "TextChannel") {
        return Ok(false);
    }
    module(lua, chat)?.get::<Function>("set")?.call((
        class,
        instance(lua, Some(owner))?,
        key,
        value.clone(),
    ))
}

/// A chat packet arriving: on the server from `from`'s client, on a client
/// from the server.
pub(super) fn receive(lua: &Lua, chat: &Chat, from: Option<Ref>, packet: Packet) -> Result<()> {
    let Packet::Chat {
        channel,
        text,
        id,
        source,
    } = packet
    else {
        return Ok(());
    };
    let module = module(lua, chat)?;
    let exists = engine(lua).ctx.dom().get(channel).is_some();
    let channel = instance(lua, exists.then_some(channel))?;
    if chat.client.get() {
        let source = instance(lua, source)?;
        module
            .get::<Function>("fromServer")?
            .call((channel, text, id, source))
    } else {
        module
            .get::<Function>("fromClient")?
            .call((instance(lua, from)?, channel, text, id))
    }
}

/// The editor's chat bar: the local player says `text` in the general
/// channel.
pub(super) fn say(lua: &Lua, chat: &Chat, text: &str) -> Result<()> {
    module(lua, chat)?.get::<Function>("say")?.call(text)
}
