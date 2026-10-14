//! Wire format between the in-game probe and the editor. JSON over HTTPS,
//! the token travels in the `X-Probe-Token` header on every request.
//!
//! - Uplink: `POST /v1/ingest`, body = JSON array of [`Uplink`].
//! - Downlink: `GET /v1/poll?since=<id>`, held open until a [`Command`] with a
//!   larger id exists; answers `200` with a JSON array of them, or `204`.
//! - Health: `GET /healthz` answers `200 ok`.
//! - Any request without the right token answers `404`.

use serde::{Deserialize, Serialize};

/// Header carrying the session token.
pub const TOKEN_HEADER: &str = "X-Probe-Token";

/// The per-Play secret: 32 random bytes as 64 lowercase hex chars.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// A fresh token from the OS RNG.
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("OS random number generator");
        Token(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Constant-time comparison against a header value.
    pub fn matches(&self, presented: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), presented.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// Which side of the game a message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Server,
    Client,
}

/// `LogService.MessageOut`'s `MessageType`, lowercased.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Output,
    Info,
    Warning,
    Error,
}

/// Probe → editor. Tagged by `"type"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Uplink {
    /// First message of a server; also the flag self-check.
    Hello {
        session: String,
        place_version: u64,
        http_enabled: bool,
        loadstring_enabled: bool,
        is_studio: bool,
        /// `game.JobId`: the server instance, for a collaborator's join link.
        #[serde(default)]
        job_id: String,
    },
    Log {
        side: Side,
        level: LogLevel,
        message: String,
        /// Seconds since the probe started.
        time: f64,
    },
    /// Result of a command: `ok` plus a printable value or error text.
    Ack { id: u64, ok: bool, result: String },
    /// One side's performance, about once a second (see `Metrics`).
    Stats { side: Side, metrics: Metrics },
    /// A player joined the server (`Players.PlayerAdded`, and every player
    /// already in when the probe starts).
    Joined { user_id: u64 },
    /// A player left the server (`Players.PlayerRemoving`). The editor stops
    /// the Play session when the developer's own player leaves.
    Left { user_id: u64 },
    /// Server is shutting down (`game:BindToClose`).
    Bye,
    /// A chunk of one side's whole instance tree, sent after the probe
    /// starts and on `Resync`. `reset` on the first chunk (drop the editor's
    /// copy), `done` on the last. Parents come before their children.
    Tree {
        side: Side,
        reset: bool,
        done: bool,
        items: Vec<Node>,
    },
    /// Structural changes on one side since its last message, in the order
    /// they happened.
    Diff {
        side: Side,
        #[serde(default)]
        added: Vec<Node>,
        #[serde(default)]
        removed: Vec<u64>,
        #[serde(default)]
        renamed: Vec<Rename>,
        #[serde(default)]
        moved: Vec<Move>,
    },
    /// Property values of one watched instance (see `CommandBody::Watch`):
    /// every watched property the first time, then only those that changed.
    /// Values use the encoding in `crate::value`; a property that could not
    /// be read is absent.
    Props {
        side: Side,
        id: u64,
        #[serde(default)]
        values: std::collections::BTreeMap<String, serde_json::Value>,
        /// All attributes, when they changed (or on the first message).
        #[serde(default)]
        attributes: Option<std::collections::BTreeMap<String, serde_json::Value>>,
        /// `CollectionService` tags, when they changed (or first).
        #[serde(default)]
        tags: Option<Vec<String>>,
    },
}

/// A side's performance numbers, read from Roblox's `Stats` service and
/// friends; a number the side could not read is `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Client: frames rendered in the last second (`RenderStepped`).
    /// Server: heartbeats in the last second (`Heartbeat`).
    #[serde(default)]
    pub fps: Option<f64>,
    /// Server only: `workspace:GetRealPhysicsFPS()`.
    #[serde(default)]
    pub physics_fps: Option<f64>,
    /// `Stats:GetTotalMemoryUsageMb()`.
    #[serde(default)]
    pub memory_mb: Option<f64>,
    /// `Stats.InstanceCount`.
    #[serde(default)]
    pub instances: Option<u64>,
    /// `Stats.DataSendKbps` / `Stats.DataReceiveKbps`.
    #[serde(default)]
    pub send_kbps: Option<f64>,
    #[serde(default)]
    pub receive_kbps: Option<f64>,
    /// Server: `Stats.HeartbeatTime`; client: `Stats.FrameTime` — in
    /// milliseconds (Roblox reports seconds).
    #[serde(default)]
    pub frame_ms: Option<f64>,
    /// Client only: `LocalPlayer:GetNetworkPing()`, round trip, in ms.
    #[serde(default)]
    pub ping_ms: Option<f64>,
    /// Server only: `#Players:GetPlayers()`.
    #[serde(default)]
    pub players: Option<u32>,
}

/// One instance in a side's tree. Ids are per side, assigned by that side's
/// probe, never reused; `parent == 0` means a service (a child of `game`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    pub parent: u64,
    pub class: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub id: u64,
    pub parent: u64,
}

/// One instance whose properties the editor shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchItem {
    pub id: u64,
    /// Property names to read (the editor's Properties panel list).
    pub props: Vec<String>,
}

/// Editor → probe. `id` is assigned by the channel, strictly increasing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    #[serde(flatten)]
    pub body: CommandBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CommandBody {
    /// Run Luau on the server through `loadstring`; the ack carries the
    /// `tostring` of its first return value.
    Eval { source: String },
    /// Replace a server Script's source and re-run it (hot reload).
    Reload { path: String, source: String },
    /// `game:Shutdown()`-like: kick everyone, then stop polling.
    Shutdown,
    /// Replaces `side`'s watch set: the probe answers `Props` for each item
    /// at once, then re-reads them every 0.25 s and sends what changed
    /// (polled, because `Changed` misses physics-driven properties).
    Watch { side: Side, items: Vec<WatchItem> },
    /// `instance[prop] = value` on `side` (value in `crate::value`'s encoding).
    Set {
        side: Side,
        target: u64,
        prop: String,
        value: serde_json::Value,
    },
    /// `instance:SetAttribute(name, value)`; `null` removes it.
    #[serde(rename = "set_attribute")]
    SetAttribute {
        side: Side,
        target: u64,
        name: String,
        value: serde_json::Value,
    },
    /// `instance:Destroy()`.
    Destroy { side: Side, target: u64 },
    /// `instance.Parent = parent` (`0` = nil).
    Reparent {
        side: Side,
        target: u64,
        parent: u64,
    },
    /// `Instance.new(class)` named `name` under `parent`.
    Create {
        side: Side,
        class: String,
        name: String,
        parent: u64,
    },
    /// Makes `instance:GetTags()` equal `tags`: `AddTag` each missing one,
    /// `RemoveTag` each extra one.
    #[serde(rename = "set_tags")]
    SetTags {
        side: Side,
        target: u64,
        tags: Vec<String>,
    },
    /// Send `side`'s whole tree again (`Tree` with `reset`).
    Resync { side: Side },
}
