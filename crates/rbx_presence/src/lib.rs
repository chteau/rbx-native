//! Team presence through a relay: what collaborators running the
//! rbx-native presence plugin in Roblox Studio, and rbx-native users, tell
//! each other about where they are and what they have selected.
//!
//! This is the official-API route. Nothing here speaks Roblox's own Team
//! Create protocol: Studio's side is a plugin (`plugins/team-presence/`) that
//! talks to a relay (`relay/`) over `HttpService`, and this crate is the
//! editor's side of the same relay. Presence therefore only exists for a
//! collaborator while they have Studio open with the plugin running.
//!
//! - [`schema`]: the one JSON shape both directions use.
//! - [`auth`]: the per-place key, room id and request signatures.
//! - [`crypt`] / [`seal`]: payload encryption; the relay sees only ciphertext.
//! - [`paths`]: instances named by their full path from `game`.
//! - [`secret`]: where the per-place secret lives inside the place.
//! - [`relay`]: the default endpoint both sides connect to with no config.
//! - [`coedit`]: live co-editing, ops through the relay's op log.
//! - [`client`]: the relay WebSocket, on a worker thread.
//! - [`host`]: the relay itself, run by rbx-native behind a Cloudflare quick tunnel.
//! - [`announce`]: where the room's host is, posted through ntfy.
//! - [`catchup`]: a summary of the value-less edit journal for a reconnecting user.
//! - [`lastseen`]: the per-place timestamp and snapshot a reconnecting user catches up from.

/// Unix milliseconds now, or 0 if the clock is before the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub mod announce;
pub mod auth;
pub mod catchup;
pub mod client;
pub mod coedit;
pub mod crypt;
pub mod host;
pub mod lastseen;
pub mod ntfy;
pub mod paths;
pub mod relay;
pub mod schema;
pub mod seal;
pub mod secret;
