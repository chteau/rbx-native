//! Play/Test without Roblox Studio: the editor-side half of the sandbox
//! design in `docs/research/play-sandbox/README.md`.
//!
//! - `protocol`: the JSON messages the in-game probe and the editor exchange.
//! - `channel`: the loopback HTTP server the probe talks to (through a tunnel).
//! - `tunnel`: starts a public tunnel to that server and scrapes its URL.
//! - `inject`: adds the probe and the flags it needs to the DOM before publish.
//! - `instrument`: rewrites a script with debugger hooks for Play.
//! - `launch`: the `roblox://` join URI and the program that opens it.
//! - `value`: Roblox values ↔ JSON, both ways, for live properties.
//! - `live`: a side's live instance tree as a `WeakDom`, from `Tree`/`Diff`/`Props`.
//! - `embed`: puts the client's window, frameless, into the viewport (Linux
//!   X11 and Windows).

pub mod channel;
#[cfg(any(target_os = "linux", windows))]
pub mod embed;
pub mod inject;
pub mod instrument;
pub mod launch;
pub mod live;
pub mod protocol;
pub mod tunnel;
pub mod value;
