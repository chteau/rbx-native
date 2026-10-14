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
    /// Players, memory, fps; free-form until the UI needs a shape.
    Stats { data: serde_json::Value },
    /// A player joined the server (`Players.PlayerAdded`, and every player
    /// already in when the probe starts).
    Joined { user_id: u64 },
    /// A player left the server (`Players.PlayerRemoving`). The editor stops
    /// the Play session when the developer's own player leaves.
    Left { user_id: u64 },
    /// Server is shutting down (`game:BindToClose`).
    Bye,
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
}
