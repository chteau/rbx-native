//! Where presence connects, and over which transport.
//!
//! Zero-config connect rests on the place itself naming the relay: the place
//! owner sets it once (see [`crate::secret`], which keeps it beside the
//! secret on `ServerStorage/RbxNativePresence`), and every collaborator
//! reads it from the place and just clicks Connect. A place that names no
//! relay falls back to the [`community`](RelayConfig::community) instance —
//! best effort, no uptime promise.
//!
//! Three transports are supported, all official:
//! - [`RelayKind::Hosted`] (the default): no server and no account. The
//!   first rbx-native user in the room runs the relay itself
//!   ([`crate::host`]) behind a Cloudflare quick tunnel and announces its
//!   address on an ntfy topic ([`crate::announce`]); everyone else, Studio
//!   included, reads the address there and connects to it. The URL is the
//!   ntfy server the announcements go through.
//! - [`RelayKind::Worker`]: the signed-WebSocket relay in `relay/` (a
//!   Cloudflare Worker), which keeps a room in memory and fans out snapshots.
//! - [`RelayKind::Ntfy`]: a plain [ntfy](https://ntfy.sh) pub/sub server,
//!   which needs no code of ours at all. Each client publishes its signed
//!   presence to a topic derived from the room and reconstructs the room from
//!   the message stream; peers trust each other by HMAC, never the server.
//!   Public ntfy.sh allows 250 messages a day per address, far too few for
//!   live presence; this needs a self-hosted ntfy.

/// Which transport a relay endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayKind {
    /// An rbx-native user hosts the relay; announced through ntfy.
    Hosted,
    /// The custom signed-WebSocket relay in `relay/`.
    Worker,
    /// A generic ntfy pub/sub server.
    Ntfy,
}

impl RelayKind {
    /// The lower-case word stored in the place and on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            RelayKind::Hosted => "hosted",
            RelayKind::Worker => "worker",
            RelayKind::Ntfy => "ntfy",
        }
    }

    /// The kind for `word`, or `None` for anything else.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "hosted" => Some(RelayKind::Hosted),
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

/// The fallback used when the place names no relay: hosted by whoever opens
/// the place in rbx-native, announced through public ntfy.sh (a few messages
/// a day). Override the announce server at build time with
/// `RBX_PRESENCE_COMMUNITY_URL`.
pub const COMMUNITY_URL: &str = match option_env!("RBX_PRESENCE_COMMUNITY_URL") {
    Some(url) => url,
    None => "https://ntfy.sh",
};
pub const COMMUNITY_KIND: RelayKind = RelayKind::Hosted;
pub const COMMUNITY_LABEL: &str =
    "hosted by an rbx-native collaborator through a Cloudflare quick tunnel; no account needed";

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
