//! Remote traffic as it crosses the link, for the editor's Remote
//! inspector: every `RemoteEvent`, `UnreliableRemoteEvent` and
//! `RemoteFunction` call, each way, with the arguments as they travel
//! ([`Wire`]: instances by reference, tables re-keyed), the bytes the link
//! counts for it (the same figure `Stats` meters) and what became of it.
//!
//! Off unless [`crate::Session::capture_remotes`] turned it on: then the
//! VMs note each call's script line and the link reports losses.

use std::collections::HashMap;

use rbx_dom::{Ref, WeakDom};
use rbx_lua::server::{Caller, Packet, To, Wire};

/// Which way a call went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `FireServer` / `InvokeServer`, and a client's reply to `InvokeClient`.
    ToServer,
    /// `FireClient` / `InvokeClient`, and the server's reply to `InvokeServer`.
    ToClient,
    /// `FireAllClients`: one call per client.
    ToAll,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::ToServer => "client → server",
            Direction::ToClient => "server → client",
            Direction::ToAll => "server → all",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Unreliable,
    Invoke,
}

/// What became of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    Delivered,
    /// Reliable traffic the link lost this many times before it got through.
    Resent(u32),
    /// An unreliable packet the link lost.
    Lost,
    /// An `UnreliableRemoteEvent` payload over 1000 bytes: it never left.
    OverLimit,
    /// Over the ~500 unreliable packets a second a server takes from one
    /// client: the server dropped it.
    RateLimited,
}

/// One argument, laid out for reading: its key in a table, its type and a
/// one-line value; a table's entries under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub key: Option<String>,
    pub kind: String,
    pub text: String,
    pub children: Vec<Arg>,
}

/// A `RemoteFunction` invocation's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// From the invocation leaving to the reply arriving.
    pub seconds: f64,
    /// The callback's error, which the invoker raises.
    pub error: Option<String>,
    pub bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteCall {
    /// Numbers calls in the order they were made.
    pub id: u64,
    /// Server seconds when it left.
    pub time: f64,
    pub direction: Direction,
    pub kind: Kind,
    pub remote: Ref,
    /// The remote's full name on the sending side.
    pub path: String,
    /// The client's player at the other end.
    pub player: String,
    pub args: Vec<Arg>,
    /// The packet as the link counts it: an 8 byte header and each value's
    /// type byte and fields ([`Wire::size`]).
    pub bytes: usize,
    pub caller: Option<Caller>,
    pub fate: Fate,
}

/// What the log has to say since it was last read.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Call(RemoteCall),
    /// A call's fate learnt after it left (the server's rate limit).
    Fate(u64, Fate),
    Replied(u64, Reply),
}

/// The link's packet size for `packet` (an 8 byte header, then its values).
pub(crate) fn packet_size(packet: &Packet) -> usize {
    let wires = |w: &[Wire]| w.iter().map(Wire::size).sum::<usize>();
    8 + match packet {
        Packet::Event { args, .. } | Packet::Invoke { args, .. } => wires(args),
        Packet::Reply { result, .. } => match result {
            Ok(values) => wires(values),
            Err(message) => message.len(),
        },
        Packet::Chat { text, id, .. } => text.len() + id.len(),
        Packet::Drag { phase, numbers, .. } => phase.len() + 8 * numbers.len(),
    }
}

/// `GetFullName` in `dom`: names from under the DataModel down.
pub(crate) fn full_name(dom: &WeakDom, target: Ref) -> String {
    let mut names = Vec::new();
    let mut at = Some(target);
    while let Some(instance) = at.and_then(|r| dom.get(r)) {
        if instance.class() == "DataModel" {
            break;
        }
        names.push(instance.name().to_string());
        at = at.and_then(|r| dom.parent(r));
    }
    names.reverse();
    names.join(".")
}

/// `args` laid out for reading; instances named from `sender`, and marked
/// when `receiver` lacks them (they arrive as nil).
fn layout(args: &[Wire], sender: &WeakDom, receiver: &WeakDom) -> Vec<Arg> {
    args.iter()
        .map(|w| arg(None, w, sender, receiver))
        .collect()
}

fn arg(key: Option<String>, wire: &Wire, sender: &WeakDom, receiver: &WeakDom) -> Arg {
    let leaf = |kind: &str, text: String| Arg {
        key: key.clone(),
        kind: kind.to_owned(),
        text,
        children: Vec::new(),
    };
    match wire {
        Wire::Nil => leaf("nil", "nil".into()),
        Wire::Bool(b) => leaf("boolean", b.to_string()),
        Wire::Number(n) => leaf("number", number(*n)),
        Wire::String(bytes) => leaf("string", format!("{:?}", String::from_utf8_lossy(bytes))),
        Wire::Buffer(bytes) => leaf("buffer", format!("{} bytes", bytes.len())),
        Wire::Instance(r) => {
            let mut text = full_name(sender, *r);
            if receiver.get(*r).is_none() {
                text += "  (nil on arrival: not replicated)";
            }
            leaf("Instance", text)
        }
        Wire::Enum {
            enum_name, item, ..
        } => leaf("EnumItem", format!("Enum.{enum_name}.{item}")),
        Wire::Data(value, kind) => leaf(kind, data(value)),
        Wire::Table(entries) => {
            let children: Vec<Arg> = entries
                .iter()
                .map(|(k, v)| {
                    let k = match k {
                        Wire::String(s) => String::from_utf8_lossy(s).into_owned(),
                        Wire::Number(n) => format!("[{}]", number(*n)),
                        other => format!("[{other:?}]"),
                    };
                    arg(Some(k), v, sender, receiver)
                })
                .collect();
            Arg {
                key,
                kind: "table".into(),
                text: format!("{{{} entries}}", children.len()),
                children,
            }
        }
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn data(value: &rbx_dom::Variant) -> String {
    use rbx_dom::Variant;
    match value {
        Variant::Vector3(v) => format!("{}, {}, {}", v.x, v.y, v.z),
        Variant::Vector2(v) => format!("{}, {}", v.x, v.y),
        Variant::Color3(c) => format!("{}, {}, {}", c.r, c.g, c.b),
        Variant::CFrame(c) => format!("{}, {}, {}, …", c.position.x, c.position.y, c.position.z),
        other => format!("{other:?}"),
    }
}

/// Seconds an invocation may wait for its reply before it is flagged.
pub const LONG_INVOKE: f64 = 1.0;

#[derive(Default)]
pub(crate) struct RemoteLog {
    events: Vec<Event>,
    next: u64,
    /// Invocations waiting for their reply, by (invoker, id): the call and
    /// the server second it left.
    waiting: HashMap<(Option<Ref>, u32), (u64, f64)>,
}

/// One end of a call as the log sees it.
pub(crate) struct Ends<'a> {
    pub sender: &'a WeakDom,
    pub receiver: &'a WeakDom,
    /// The client at the other end: its player and name.
    pub player: Option<Ref>,
    pub name: &'a str,
    pub time: f64,
}

impl RemoteLog {
    pub(crate) fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Notes `packet` leaving; `None` for what isn't a remote call. Replies
    /// complete their invocation (`latency`: the reply's way, which it
    /// still has to travel).
    pub(crate) fn sent(
        &mut self,
        ends: &Ends,
        to: To,
        caller: Option<Caller>,
        packet: &Packet,
        fate: Fate,
        latency: f64,
    ) -> Option<u64> {
        let to_server = matches!(to, To::Server);
        let (remote, args, kind) = match packet {
            Packet::Event {
                remote,
                args,
                unreliable,
            } => (
                *remote,
                args,
                if *unreliable {
                    Kind::Unreliable
                } else {
                    Kind::Event
                },
            ),
            Packet::Invoke { remote, args, id } => {
                let invoker = if to_server { ends.player } else { None };
                self.waiting.insert((invoker, *id), (self.next, ends.time));
                (*remote, args, Kind::Invoke)
            }
            Packet::Reply { id, result } => {
                // A reply to the server came from a client, so the server
                // invoked it, and the other way round.
                let invoker = if to_server { None } else { ends.player };
                if let Some((call, left)) = self.waiting.remove(&(invoker, *id)) {
                    self.events.push(Event::Replied(
                        call,
                        Reply {
                            seconds: ends.time - left + latency,
                            error: result.as_ref().err().cloned(),
                            bytes: packet_size(packet),
                        },
                    ));
                }
                return None;
            }
            Packet::Chat { .. } | Packet::Drag { .. } => return None,
        };
        let id = self.next;
        self.next += 1;
        self.events.push(Event::Call(RemoteCall {
            id,
            time: ends.time,
            direction: match to {
                To::Server => Direction::ToServer,
                To::Player(_) => Direction::ToClient,
                To::AllPlayers => Direction::ToAll,
            },
            kind,
            remote,
            path: full_name(ends.sender, remote),
            player: ends.name.to_owned(),
            args: layout(args, ends.sender, ends.receiver),
            bytes: packet_size(packet),
            caller,
            fate,
        }));
        Some(id)
    }

    pub(crate) fn fate(&mut self, call: u64, fate: Fate) {
        self.events.push(Event::Fate(call, fate));
    }

    /// A reliable frame the link resent: every call in it waited.
    pub(crate) fn resent(&mut self, calls: &[u64], times: u32) {
        if times > 0 {
            for call in calls {
                self.fate(*call, Fate::Resent(times));
            }
        }
    }
}
