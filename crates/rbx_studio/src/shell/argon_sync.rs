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
/// `connection::needs_review`.
pub(super) struct PendingReview {
    pub(super) additions: usize,
    pub(super) updates: usize,
    pub(super) removals: usize,
    /// Counts up per review, so a Diff window's caches know when the
    /// batch under them changed.
    serial: u64,
    changes: argon_client::Changes,
}

impl PendingReview {
    pub(super) fn new(changes: argon_client::Changes, serial: u64) -> Self {
        PendingReview {
            additions: changes.additions.len(),
            updates: changes.updates.len(),
            removals: changes.removals.len(),
            serial,
            changes,
        }
    }

    pub(super) fn serial(&self) -> u64 {
        self.serial
    }

    pub(super) fn changes(&self) -> &argon_client::Changes {
        &self.changes
    }
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
    /// The serial the next `PendingReview` takes.
    review_serial: u64,
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
            review_serial: 0,
            write_generation: 0,
        }
    }
}

/// Where the `argon` CLI is on this machine, if anywhere — see `cli`.
pub(super) fn argon_cli_path() -> Option<std::path::PathBuf> {
    cli::locate()
}

impl Shell {
    pub(super) fn argon_state(&self) -> &SyncState {
        &self.argon.state
    }

    /// The connected project's name, or "Argon" while not connected.
    pub(in crate::shell) fn argon_project_name(&self) -> String {
        match &self.argon.state {
            SyncState::Connected { project, .. } => project.clone(),
            _ => "Argon".to_owned(),
        }
    }

    /// The Diff Lines Limit setting at the level in force.
    pub(in crate::shell) fn argon_diff_lines_limit(&self) -> usize {
        let keys = self.argon_level_keys();
        match self
            .argon_settings
            .get(crate::settings::argon::Setting::DiffLinesLimit, &keys)
        {
            crate::settings::argon::Value::Number(n) => n as usize,
            _ => 3000,
        }
    }

    /// A value in the Properties panel's own words.
    pub(in crate::shell) fn format_property(
        &self,
        class: &str,
        name: &str,
        value: &rbx_dom::Variant,
    ) -> String {
        self.properties.format(&self.dom, class, name, value)
    }

    /// Which review is pending, for a cache keyed on it.
    pub(in crate::shell) fn argon_pending_serial(&self) -> Option<u64> {
        self.argon.pending.as_ref().map(PendingReview::serial)
    }

    /// The pending review's batch, wrapped and numbered.
    pub(super) fn set_pending(&mut self, changes: argon_client::Changes) {
        self.argon.review_serial += 1;
        self.argon.pending = Some(PendingReview::new(changes, self.argon.review_serial));
    }

    pub(super) fn argon_pending(&self) -> Option<&PendingReview> {
        self.argon.pending.as_ref()
    }
}

mod apply;
mod cli;
mod connection;
mod diff_fixture;
mod diff_rows;
mod initial;
mod outgoing;

pub(in crate::shell) use diff_rows::{ChangeKind, DiffNode};

#[cfg(test)]
mod tests;
