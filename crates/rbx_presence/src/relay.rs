//! Where presence connects, and over which transport.
//!
//! Zero-config connect rests on the place itself naming the relay: the place
//! owner sets it once (see [`crate::secret`], which keeps it beside the
//! secret on `ServerStorage/RbxNativePresence`), and every collaborator
//! reads it from the place and just clicks Connect. A place that names no
//! relay falls back to the [`community`](RelayConfig::community) instance —
//! best effort, no uptime promise.
//!
//! Two transports are supported, both official:
//! - [`RelayKind::Worker`]: the signed-WebSocket relay in `relay/` (a
//!   Cloudflare Worker), which keeps a room in memory and fans out snapshots.
//! - [`RelayKind::Ntfy`]: a plain [ntfy](https://ntfy.sh) pub/sub server,
//!   which needs no code of ours at all. Each client publishes its signed
//!   presence to a topic derived from the room and reconstructs the room from
//!   the message stream; peers trust each other by HMAC, never the server.

/// Which transport a relay endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayKind {
    /// The custom signed-WebSocket relay in `relay/`.
    Worker,
    /// A generic ntfy pub/sub server.
    Ntfy,
}

impl RelayKind {
    /// The lower-case word stored in the place and on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            RelayKind::Worker => "worker",
            RelayKind::Ntfy => "ntfy",
        }
    }

    /// The kind for `word`, or `None` for anything else.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "worker" => Some(RelayKind::Worker),
            "ntfy" => Some(RelayKind::Ntfy),
            _ => None,
        }
    }
}

/// A resolved relay: the transport and its base URL
/// (`https://host[/prefix]`, trailing `/` ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConfig {
    pub kind: RelayKind,
    pub url: String,
}

/// The community fallback used when the place names no relay. A public ntfy
/// instance so there is nothing to host; the UI labels it [`COMMUNITY_LABEL`]
/// so no one mistakes it for a maintained service. Override at build time
/// with `RBX_PRESENCE_COMMUNITY_URL`.
pub const COMMUNITY_URL: &str = match option_env!("RBX_PRESENCE_COMMUNITY_URL") {
    Some(url) => url,
    None => "https://ntfy.sh",
};
pub const COMMUNITY_KIND: RelayKind = RelayKind::Ntfy;
pub const COMMUNITY_LABEL: &str = "community instance, best effort, no uptime promise";

impl RelayConfig {
    /// The community fallback ([`COMMUNITY_KIND`] at [`COMMUNITY_URL`]).
    pub fn community() -> Self {
        RelayConfig {
            kind: COMMUNITY_KIND,
            url: COMMUNITY_URL.to_owned(),
        }
    }

    /// `url` with a trailing `/` removed.
    pub fn trimmed_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }
}

/// The ntfy topic for a room: a short, unguessable name derived from the room
/// id (itself derived from the place secret), so only collaborators who hold
/// the secret know where to listen on a shared public server.
pub fn ntfy_topic(room: &str) -> String {
    format!("rbxnp-{room}")
}
