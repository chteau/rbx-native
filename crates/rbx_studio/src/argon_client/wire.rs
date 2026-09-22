//! Envelope types for Argon's wire protocol — `Message`, `Project`,
//! `Snapshot`, `Changes` — decoded from an [`rmpv::Value`] tree by field
//! name, the shapes `argon-roblox`'s `src/Types.luau` defines. Property
//! *values* are left undecoded here (`Vec<(String, Value)>`): turning an
//! `EncodedValue` into an `rbx_dom::Variant` is [`super::value`]'s job, kept
//! separate so this module has no `rbx_dom` dependency of its own.

use rmpv::Value;

/// Argon's own instance identity: 16 random bytes
/// (`argon-roblox/src/Helpers/generateRef.luau`), sent as a raw MsgPack
/// `bin`. Distinct from `rbx_dom::Ref`, which is a small integer local to
/// one `WeakDom` — `shell::argon_sync` bridges the two with an id map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ArgonRef(pub(crate) [u8; 16]);

impl ArgonRef {
    /// What an empty/all-zero buffer means on the wire: "the DataModel
    /// itself," used as the root of a `snapshot` request.
    pub(crate) const ROOT: ArgonRef = ArgonRef([0; 16]);

    fn decode(value: &Value) -> Option<Self> {
        let bytes = value.as_slice()?;
        (bytes.len() == 16).then(|| {
            let mut out = [0u8; 16];
            out.copy_from_slice(bytes);
            ArgonRef(out)
        })
    }

    pub(crate) fn encode(self) -> Value {
        Value::Binary(self.0.to_vec())
    }

    /// A fresh id for an instance this client is adding, the same way
    /// `generateRef.luau` makes one: 16 uniformly random bytes.
    pub(crate) fn generate() -> Self {
        let mut bytes = [0u8; 16];
        for byte in &mut bytes {
            *byte = rand_byte();
        }
        ArgonRef(bytes)
    }
}

// No `rand` dependency for 16 bytes a session only needs to be locally
// unique against a HashMap key, not cryptographically unbiased — a tiny
// xorshift seeded from the system clock is plenty and costs no new crate.
fn rand_byte() -> u8 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static STATE: Cell<u64> = Cell::new(
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x2545F4914F6CDD1D) | 1
        );
    }
    STATE.with(|state| {
        let mut x = state.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        (x & 0xFF) as u8
    })
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

fn get_str(value: &Value, key: &str) -> Option<String> {
    map_get(value, key)?.as_str().map(str::to_owned)
}

/// The project a `GET details` (or an inbound `SyncDetails` message)
/// reports — `argon-roblox`'s `Types.Project`.
pub(crate) struct Project {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl Project {
    pub(crate) fn decode(value: &Value) -> Option<Self> {
        Some(Project {
            name: get_str(value, "name")?,
            version: get_str(value, "version")?,
        })
    }
}

/// One instance, and (for `additions`) where it attaches. `meta` (a mesh
/// source override, mostly) isn't acted on in this client yet — decoded and
/// dropped, same as an unrecognized property tag falls to
/// `rbx_dom::Variant::Unknown` rather than refusing the whole snapshot.
pub(crate) struct Snapshot {
    pub(crate) id: ArgonRef,
    pub(crate) parent: Option<ArgonRef>,
    pub(crate) name: String,
    pub(crate) class: String,
    pub(crate) properties: Vec<(String, Value)>,
    pub(crate) children: Vec<Snapshot>,
}

impl Snapshot {
    pub(crate) fn decode(value: &Value) -> Option<Self> {
        let id = ArgonRef::decode(map_get(value, "id")?)?;
        let parent = map_get(value, "parent").and_then(ArgonRef::decode);
        let name = get_str(value, "name")?;
        let class = get_str(value, "class")?;
        let properties = decode_properties(map_get(value, "properties")?);
        let children = map_get(value, "children")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Snapshot::decode).collect())
            .unwrap_or_default();
        Some(Snapshot {
            id,
            parent,
            name,
            class,
            properties,
            children,
        })
    }
}

/// A sparse update: only `id` is guaranteed present, matching
/// `argon-roblox`'s `Types.UpdatedSnapshot` — everything else is `None`
/// when that instance's update didn't touch it.
pub(crate) struct UpdatedSnapshot {
    pub(crate) id: ArgonRef,
    pub(crate) name: Option<String>,
    pub(crate) class: Option<String>,
    pub(crate) properties: Option<Vec<(String, Value)>>,
}

impl UpdatedSnapshot {
    fn decode(value: &Value) -> Option<Self> {
        Some(UpdatedSnapshot {
            id: ArgonRef::decode(map_get(value, "id")?)?,
            name: get_str(value, "name"),
            class: get_str(value, "class"),
            properties: map_get(value, "properties").map(decode_properties),
        })
    }
}

/// One `properties` map decoded into `(name, EncodedValue)` pairs — with
/// the `ArgonEmpty` sentinel key (`argon-roblox`'s workaround for an empty
/// Luau table serializing as an array, not a map) dropped, since it names
/// no real property.
fn decode_properties(value: &Value) -> Vec<(String, Value)> {
    value
        .as_map()
        .into_iter()
        .flatten()
        .filter_map(|(key, val)| {
            let name = key.as_str()?;
            (name != "ArgonEmpty").then(|| (name.to_owned(), val.clone()))
        })
        .collect()
}

pub(crate) struct Changes {
    pub(crate) additions: Vec<Snapshot>,
    pub(crate) updates: Vec<UpdatedSnapshot>,
    pub(crate) removals: Vec<ArgonRef>,
}

impl Changes {
    pub(crate) fn decode(value: &Value) -> Option<Self> {
        let additions = map_get(value, "additions")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Snapshot::decode).collect())
            .unwrap_or_default();
        let updates = map_get(value, "updates")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(UpdatedSnapshot::decode).collect())
            .unwrap_or_default();
        let removals = map_get(value, "removals")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(ArgonRef::decode).collect())
            .unwrap_or_default();
        Some(Changes {
            additions,
            updates,
            removals,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.updates.is_empty() && self.removals.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.additions.len() + self.updates.len() + self.removals.len()
    }

    pub(crate) fn encode(&self) -> Value {
        Value::Map(vec![
            (
                Value::from("additions"),
                Value::Array(self.additions.iter().map(Snapshot::encode).collect()),
            ),
            (
                Value::from("updates"),
                Value::Array(self.updates.iter().map(UpdatedSnapshot::encode).collect()),
            ),
            (
                Value::from("removals"),
                Value::Array(self.removals.iter().map(|r| r.encode()).collect()),
            ),
        ])
    }
}

impl Snapshot {
    /// The wire shape one `additions` entry takes on write-back: an empty
    /// `properties` map is encoded as the same `ArgonEmpty` sentinel
    /// `decode_properties` strips on the way in, since a genuinely empty
    /// MsgPack map round-trips through Argon's own Luau decoder as an
    /// (indistinguishable) empty array otherwise.
    fn encode(&self) -> Value {
        let properties = if self.properties.is_empty() {
            Value::Map(vec![(
                Value::from("ArgonEmpty"),
                Value::Map(vec![(Value::from("Bool"), Value::from(true))]),
            )])
        } else {
            Value::Map(
                self.properties
                    .iter()
                    .map(|(name, val)| (Value::from(name.as_str()), val.clone()))
                    .collect(),
            )
        };
        let mut fields = vec![
            (Value::from("id"), self.id.encode()),
            (Value::from("name"), Value::from(self.name.as_str())),
            (Value::from("class"), Value::from(self.class.as_str())),
            (Value::from("properties"), properties),
            (
                Value::from("children"),
                Value::Array(self.children.iter().map(Snapshot::encode).collect()),
            ),
        ];
        if let Some(parent) = self.parent {
            fields.push((Value::from("parent"), parent.encode()));
        }
        Value::Map(fields)
    }
}

impl UpdatedSnapshot {
    fn encode(&self) -> Value {
        let mut fields = vec![(Value::from("id"), self.id.encode())];
        if let Some(name) = &self.name {
            fields.push((Value::from("name"), Value::from(name.as_str())));
        }
        if let Some(class) = &self.class {
            fields.push((Value::from("class"), Value::from(class.as_str())));
        }
        if let Some(properties) = &self.properties {
            fields.push((
                Value::from("properties"),
                Value::Map(
                    properties
                        .iter()
                        .map(|(name, val)| (Value::from(name.as_str()), val.clone()))
                        .collect(),
                ),
            ));
        }
        Value::Map(fields)
    }
}

/// One `/read` response: a single-key map, `{"SyncChanges": Changes}` and
/// the like — confirmed against `argon-roblox`'s actual consumer
/// (`Core.luau`'s `next(message)`), not just its type alias, since the
/// alias's `ExecuteCode`/`Disconnect` shapes look unwrapped on paper but
/// aren't on the wire.
pub(crate) enum Message {
    SyncChanges(Changes),
    SyncDetails(Project),
    /// Decoded, never acted on — see `argon_client`'s module doc: running
    /// server-sent code is a boundary this client refuses to cross.
    ExecuteCode,
    Disconnect(String),
}

impl Message {
    pub(crate) fn decode(value: &Value) -> Option<Self> {
        let (kind, payload) = value.as_map()?.first()?;
        match kind.as_str()? {
            "SyncChanges" => Some(Message::SyncChanges(Changes::decode(payload)?)),
            "SyncDetails" => Some(Message::SyncDetails(Project::decode(payload)?)),
            "ExecuteCode" => Some(Message::ExecuteCode),
            "Disconnect" => Some(Message::Disconnect(
                get_str(payload, "message").unwrap_or_default(),
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
