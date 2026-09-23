//! Wires `crate::argon_client::ArgonClient` into `Shell`: applying an
//! incoming `Changes` batch to the DOM as one undo step (the same
//! `push_history`/`take_changes`/`reflect_changes`/`record_history_change`
//! skeleton `shell::command::run_command` already uses), and — the other
//! direction — forwarding a genuine local edit back out over `POST write`.
//!
//! **Echo suppression.** `Shell::reflect_changes` is the one function every
//! local mutation already funnels through (a Properties row, an Explorer
//! insert, a script run, undo/redo — see `shell::command`'s own doc
//! comment), so it's also this module's hook for "something changed,
//! consider syncing it out." Applying a *remote* batch runs through that
//! same function, though, so [`Shell::argon_applying`] is held `true` for
//! the whole call — [`Shell::forward_to_argon`] checks it first and does
//! nothing while it's set, which is what stops a change Argon just sent
//! from being read back out as if the user had made it.
//!
//! **Id mapping.** Argon identifies instances by a 16-byte
//! [`crate::argon_client::ArgonRef`]; this DOM identifies them by a small
//! per-session `rbx_dom::Ref`. `Shell::argon_ids`/`argon_ids_rev` are the
//! two-way table connecting them, seeded from the initial snapshot (root-
//! level entries matched against this DOM's existing services by class,
//! rather than duplicated) and extended as later additions/local inserts
//! are seen.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rbx_dom::Ref;

use crate::argon_client::{self, ArgonClient, ArgonRef};
use crate::settings::argon::LevelKeys;

use super::Shell;

/// A batch this many changes or larger is held for Accept/Cancel instead of
/// applied on sight — the same default `argon-roblox`'s own
/// `Config.ChangesThreshold` uses.
const REVIEW_THRESHOLD: usize = 5;
/// How often the poll loop drains the background thread's channel — also
/// what keeps the dock's "Ns ago" readout ticking while nothing else is
/// happening.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How long a burst of local edits (a drag's many steps, a multi-property
/// commit) is left to settle before one `POST write` goes out for all of
/// them — the same debounce shape `shell::scripts`'s script-source commit
/// already uses.
const WRITE_DEBOUNCE: Duration = Duration::from_millis(300);

/// `RBX_STUDIO_ARGON_CONNECT=1` (or `=<host>:<port>` to override the dock's
/// own address field first) clicks Connect on the editor's behalf, the same
/// screenshot-aid reason every other `RBX_STUDIO_*` var in `shell.rs`
/// exists — see `main`'s module doc comment.
pub(crate) const CONNECT_VARIABLE: &str = "RBX_STUDIO_ARGON_CONNECT";

/// `RBX_STUDIO_ARGON_DIFF=1` opens the review prompt's Diff window at
/// startup — documented at [`Shell::apply_debug_argon_diff`].
pub(crate) const DIFF_VARIABLE: &str = "RBX_STUDIO_ARGON_DIFF";

/// What the Argon dock shows — `argon-roblox`'s own plugin state machine
/// (`NotConnected`/`Connecting`/`Connected`/`Error`), minus its `Unavailable`
/// page (this editor has no play-test mode to be unavailable during).
pub(super) enum SyncState {
    NotConnected,
    Connecting,
    Connected {
        project: String,
        address: String,
        last_sync: Option<Instant>,
        direction: SyncDirection,
        /// What the connected project identifies as the Game and Place
        /// levels of Argon's settings — see [`level_keys`].
        keys: LevelKeys,
    },
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SyncDirection {
    /// The server pushed something in.
    Down,
    /// A local edit was just pushed out.
    Up,
}

/// An incoming batch large enough to ask before applying — see
/// [`REVIEW_THRESHOLD`].
pub(super) struct PendingReview {
    pub(super) additions: usize,
    pub(super) updates: usize,
    pub(super) removals: usize,
    changes: argon_client::Changes,
}

/// One row of the Diff window (`shell::argon_diff_window`) — the batch's
/// own additions/updates/removals, read back out in a shape a render can
/// walk without reaching into `argon_client::Changes` itself.
pub(super) struct DiffRow {
    pub(super) kind: DiffRowKind,
    pub(super) name: String,
    pub(super) class: String,
    /// A row's own property changes. Empty for a removal — there is nothing
    /// left to compare once the instance is gone.
    pub(super) properties: Vec<PropertyDiff>,
    /// An addition's own descendant count — the batch only lists an
    /// addition's own subtree once, at its root, so the row that stands for
    /// a whole new folder of scripts says so rather than looking like one
    /// bare instance.
    pub(super) nested: usize,
}

pub(super) enum DiffRowKind {
    Addition,
    Update,
    Removal,
}

pub(super) struct PropertyDiff {
    pub(super) name: String,
    /// `None` for a property an addition is introducing for the first time
    /// — there is no "before" for a row that didn't exist a moment ago.
    pub(super) before: Option<String>,
    pub(super) after: String,
}

/// Everything `Shell` owns for one Argon connection. Not present at all
/// while disconnected — kept together so `Shell::argon_disconnect` clears
/// it in one move rather than resetting a scatter of fields.
pub(super) struct Sync {
    pub(super) state: SyncState,
    client: Option<ArgonClient>,
    pending: Option<PendingReview>,
    /// Held while a remote batch is being written into the DOM — see this
    /// module's doc comment.
    applying: bool,
    ids: HashMap<ArgonRef, Ref>,
    ids_rev: HashMap<Ref, ArgonRef>,
    /// Referents a local edit touched since the last write-back, and Argon
    /// ids a local delete removed — drained together on the next debounced
    /// flush.
    dirty: HashSet<Ref>,
    removed: Vec<ArgonRef>,
    generation: u64,
    write_generation: u64,
}

impl Default for Sync {
    fn default() -> Self {
        Sync {
            state: SyncState::NotConnected,
            client: None,
            pending: None,
            applying: false,
            ids: HashMap::new(),
            ids_rev: HashMap::new(),
            dirty: HashSet::new(),
            removed: Vec::new(),
            generation: 0,
            write_generation: 0,
        }
    }
}
impl Shell {
    pub(super) fn argon_state(&self) -> &SyncState {
        &self.argon.state
    }

    pub(super) fn argon_pending(&self) -> Option<&PendingReview> {
        self.argon.pending.as_ref()
    }
}

mod apply;
mod connection;
mod diff_rows;
mod outgoing;

#[cfg(test)]
mod tests;
