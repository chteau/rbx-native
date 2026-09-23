//! A real client for Argon (`argon-rbx/argon`, Apache-2.0)'s sync protocol —
//! the same role its own Studio plugin
//! (`github.com/argon-rbx/argon-roblox`) plays, talking HTTP+MsgPack to a
//! locally-running `argon serve` CLI, built against this codebase's own
//! `WeakDom`/GPUI chrome instead of Roblox's. `shell::argon_sync` is the
//! only consumer — this module knows nothing about `Shell`/`WeakDom`, only
//! the wire protocol and the thread that speaks it, so it's usable and
//! testable on its own.
//!
//! **One deliberate departure from the reference plugin**: Argon's protocol
//! carries an `ExecuteCode` message that runs server-sent Luau. This client
//! decodes it (see [`wire::Message`]) and always discards it — a native
//! desktop app accepting and running arbitrary code from a local network
//! message is a real attack surface in a way it isn't inside a sandboxed
//! Roblox script, and nothing in this client executes it. Every other
//! message is handled faithfully.
//!
//! Threading follows `workspace_view::pump`'s own established idiom: one
//! dedicated OS thread (`thread::run`) owns the blocking `ureq` connection
//! and long-polls `POST read`; results cross to the caller over a plain
//! `std::sync::mpsc` channel, drained non-blockingly. No async runtime.

mod thread;
mod value;
mod wire;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

pub(crate) use thread::ArgonEvent;
pub(crate) use value::{decode as decode_value, encode as encode_value};
pub(crate) use wire::{ArgonRef, Changes, Project, Snapshot, UpdatedSnapshot};

/// A live (or connecting, or winding down) connection to one `argon serve`
/// instance. Dropping it disconnects — same shape as `workspace_view::
/// pump::Pump`'s own `Drop`.
pub(crate) struct ArgonClient {
    events: Receiver<ArgonEvent>,
    writes: Sender<Changes>,
    opens: Sender<(ArgonRef, u32)>,
    stop: Arc<AtomicBool>,
}

impl ArgonClient {
    /// Spawns the background thread and starts connecting. Nothing here
    /// blocks — the caller finds out how it went via [`ArgonClient::poll`].
    /// `https` picks the scheme, as the plugin's `Https` setting does for
    /// its own client (`Core/init.luau:46`).
    pub(crate) fn connect(host: String, port: u16, https: bool) -> Self {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (write_tx, write_rx) = std::sync::mpsc::channel();
        let (open_tx, open_rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new()
            .name("rbxstudio-argon".to_owned())
            .spawn({
                let stop = Arc::clone(&stop);
                move || thread::run(host, port, https, stop, event_tx, write_rx, open_rx)
            })
            .expect("spawning the Argon sync thread");
        ArgonClient {
            events: event_rx,
            writes: write_tx,
            opens: open_tx,
            stop,
        }
    }

    /// Asks the server to open the file behind `id` in the OS editor at
    /// `line` — the plugin's `Client:open` (`Client/init.luau:132-141`),
    /// `POST /open` on the server (`argon@3dbed6d:src/server/open.rs`).
    pub(crate) fn open(&self, id: ArgonRef, line: u32) {
        let _ = self.opens.send((id, line));
    }

    /// Drains every event the background thread has reported since the
    /// last poll — called once per tick from `shell::argon_sync`'s timer
    /// loop, never blocking.
    pub(crate) fn poll(&self) -> Vec<ArgonEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }

    /// Queues a batch of local edits for the background thread to send as
    /// one `POST write` — best-effort, dropped silently if the thread has
    /// already wound down (the next poll will report `Disconnected` too).
    pub(crate) fn write(&self, changes: Changes) {
        let _ = self.writes.send(changes);
    }
}

impl Drop for ArgonClient {
    /// Flags the thread to stop and lets it go: it notices at the end of
    /// its current long-poll (up to `READ_TIMEOUT`) and unsubscribes on
    /// its way out. Joining it here would hold the UI thread for that
    /// long on every Disconnect and place close.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// Exercises the real client against a real `argon serve` — not run by
    /// default (no server to talk to in CI), but the strongest check this
    /// module has: everything else tests the wire format and value mapping
    /// against hand-built fixtures, this proves those fixtures actually
    /// match what a live server sends. Run with a project served on
    /// `localhost:8123` (`argon serve --port 8123` from any Argon project)
    /// and `cargo test -p rbx_studio --lib argon_client -- --ignored`.
    #[test]
    #[ignore = "needs a live `argon serve --port 8123` to talk to"]
    fn connects_to_a_live_server_and_receives_its_initial_snapshot() {
        let client = ArgonClient::connect("localhost".to_owned(), 8123, false);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut project_name = None;
        let mut snapshot = None;
        while Instant::now() < deadline && (project_name.is_none() || snapshot.is_none()) {
            for event in client.poll() {
                match event {
                    ArgonEvent::Connected(project) => project_name = Some(project.name),
                    ArgonEvent::InitialSnapshot(root) => snapshot = Some(root),
                    ArgonEvent::Error(message) => panic!("connect failed: {message}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(project_name.as_deref(), Some("ArgonTest"));
        let root = snapshot.expect("an initial snapshot within the deadline");
        let classes: Vec<&str> = root.children.iter().map(|c| c.class.as_str()).collect();
        assert!(
            classes.contains(&"ReplicatedStorage"),
            "expected ReplicatedStorage among {classes:?}"
        );
        assert!(
            classes.contains(&"ServerScriptService"),
            "expected ServerScriptService among {classes:?}"
        );
    }
}
