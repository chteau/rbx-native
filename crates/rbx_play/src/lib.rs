//! Play/Test without Roblox Studio: the editor-side half of the sandbox
//! design in `docs/research/play-sandbox/README.md`.
//!
//! - `protocol`: the JSON messages the in-game probe and the editor exchange.
//! - `channel`: the loopback HTTP server the probe talks to (through a tunnel).
//! - `tunnel`: starts a public tunnel to that server and scrapes its URL.
//! - `inject`: adds the probe and the flags it needs to the DOM before publish.
//! - `launch`: the `roblox://` join URI and the program that opens it.
//! - `value`: Roblox values ↔ JSON, both ways, for live properties.
//! - `live`: a side's live instance tree as a `WeakDom`, from `Tree`/`Diff`/`Props`.
//! - `embed`: re-parents the client's X11 window, frameless, into the viewport.

pub mod channel;
#[cfg(target_os = "linux")]
pub mod embed;
pub mod inject;
pub mod launch;
pub mod live;
pub mod protocol;
pub mod tunnel;
pub mod value;
