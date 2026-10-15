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
//! - [`paths`]: instances named by their full path from `game`.
//! - [`secret`]: where the per-place secret lives inside the place.
//! - [`relay`]: the default endpoint both sides connect to with no config.
//! - [`client`]: the relay WebSocket, on a worker thread.

pub mod auth;
pub mod client;
pub mod ntfy;
pub mod paths;
pub mod relay;
pub mod schema;
pub mod secret;
