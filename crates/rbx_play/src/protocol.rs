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

/// Where a game `print` was called: the chunk name Roblox gives the script
/// (its full name, `ServerScriptService.Main`) and the line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub script: String,
    pub line: u32,
}

/// One `print` argument: what `tostring` said, or a table walked into a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrintedPart {
    Text(String),
    Table(PrintedTable),
}

/// A printed table, keys sorted numbers first, then strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintedTable {
    /// `0x…`, the address `tostring` puts after `table: `.
    pub address: String,
    #[serde(default)]
    pub entries: Vec<PrintedEntry>,
    /// Entries past the probe's cap, counted but not walked.
    #[serde(default)]
    pub more: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintedEntry {
    /// The key as Studio writes it inside its brackets: `"name"` or `1`.
    pub key: String,
    pub value: PrintedValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrintedValue {
    /// A scalar, strings quoted.
    Text(String),
    Table(PrintedTable),
    /// A table already open up this branch, or past the depth cap.
    Reference {
        reference: String,
    },
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
        /// The `print` call's arguments, kept structured, when one of them
        /// was a table (`probe/print.luau`); empty otherwise.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<PrintedPart>,
        /// The game script and line that printed it, for a `print` from a
        /// game script (`crate::print`); `None` for everything else.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<Origin>,
    },
    /// Result of a command: `ok` plus a printable value or error text.
    Ack { id: u64, ok: bool, result: String },
    /// Debugger: a thread stopped at a breakpoint or after a step, in
    /// instrumented script `script` at `line`. It waits for a
    /// `CommandBody::DebugResume` naming `pause`; other threads keep running.
    Paused {
        side: Side,
        pause: u64,
        script: u32,
        line: u32,
        /// Innermost first; the first is `script` at `line`.
        frames: Vec<DebugFrame>,
        /// The locals in scope at the pause, in declaration order.
        locals: Vec<DebugVariable>,
    },
    /// Debugger: the answer to a `CommandBody::DebugWatch`.
    #[serde(rename = "debug_values")]
    DebugValues {
        side: Side,
        pause: u64,
        values: Vec<DebugValue>,
    },
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
    /// What the client says about its own screen and input, once at start
    /// and whenever any of it changes (`probe/client.luau`): the record a
    /// Device Simulator live test is checked against.
    Device(DeviceReport),
}

/// The client's view of the device it runs on. Every field is read-only to
/// game scripts, so these are the engine's own values, not ours.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceReport {
    /// `Camera.ViewportSize`, in Roblox UI offset units.
    pub viewport_width: f64,
    pub viewport_height: f64,
    /// `UserInputService.TouchEnabled` / `KeyboardEnabled` / `GamepadEnabled`.
    pub touch_enabled: bool,
    pub keyboard_enabled: bool,
    pub gamepad_enabled: bool,
    /// `GuiService:IsTenFootInterface()`.
    pub ten_foot: bool,
    /// `UserInputService.PreferredInput`'s name, where the client has it.
    pub preferred_input: Option<String>,
    /// `UserInputService.OnScreenKeyboardVisible`.
    pub on_screen_keyboard_visible: bool,
    /// A `TextBox` has the keyboard (`TextBoxFocused` until
    /// `TextBoxFocusReleased`), or the chat bar does
    /// (`ChatInputBarConfiguration.IsFocused`).
    pub text_focus: bool,
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

/// One breakpoint line of an instrumented script (`crate::instrument`'s
/// `script_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugLine {
    pub script: u32,
    pub line: u32,
}

/// What a paused thread does next. Steps follow that thread only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugAction {
    /// Run until the next breakpoint.
    Continue,
    /// Stop at the next hooked line this thread reaches, at any depth.
    StepInto,
    /// Stop at the next hooked line of this function or a caller.
    StepOver,
    /// Stop at the next hooked line of a caller.
    StepOut,
}

/// One Call Stack frame of a paused thread, from `debug.info(level, "sln")`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugFrame {
    /// The script's full name in the game (`ServerScriptService.Main`).
    pub source: String,
    pub line: u32,
    /// The function's name, empty for the script's main chunk or an
    /// anonymous function.
    #[serde(default)]
    pub function: String,
}

/// A local at a pause, formatted for display by the probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugVariable {
    pub name: String,
    pub value: String,
}

/// One watch expression's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugValue {
    pub expression: String,
    pub ok: bool,
    /// The formatted value, or the error.
    pub text: String,
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
    /// Debugger: replaces `side`'s active breakpoints. Every line of every
    /// instrumented script (`crate::instrument`) not listed is off.
    #[serde(rename = "debug_set")]
    DebugSet { side: Side, lines: Vec<DebugLine> },
    /// Debugger: lets the thread stopped at `pause` go on.
    #[serde(rename = "debug_resume")]
    DebugResume {
        side: Side,
        pause: u64,
        action: DebugAction,
    },
    /// Debugger: evaluates each expression against the locals at `pause`
    /// (and the script's globals); answered with `Uplink::DebugValues`.
    #[serde(rename = "debug_watch")]
    DebugWatch {
        side: Side,
        pause: u64,
        expressions: Vec<String>,
    },
}
