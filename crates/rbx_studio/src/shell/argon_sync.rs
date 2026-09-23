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

use gpui_kit::Context;
use rbx_dom::{Change, Ref, WeakDom};

use crate::argon_client::{self, ArgonClient, ArgonEvent, ArgonRef};
use crate::settings::argon::{LevelKeys, Setting, Value};

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

    /// The pending batch's own rows, read back off whatever this DOM (still
    /// unchanged — nothing in a pending review has been applied yet) and
    /// `self.argon.ids` currently hold, so a Diff window rebuilding this
    /// every frame (`shell::argon_diff_window`, the same "no copy of the
    /// value" rule `sequence_window` already follows) always shows the
    /// batch against what the tree actually looks like right now. Empty
    /// once nothing is pending — including right after Accept/Cancel, which
    /// is what tells that window to close itself.
    pub(super) fn argon_diff_rows(&self) -> Vec<DiffRow> {
        let Some(pending) = &self.argon.pending else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for addition in &pending.changes.additions {
            let properties = addition
                .properties
                .iter()
                .filter_map(|(name, encoded)| {
                    let after = argon_client::decode_value(encoded)?;
                    Some(PropertyDiff {
                        name: name.clone(),
                        before: None,
                        after: format!("{after:?}"),
                    })
                })
                .collect();
            rows.push(DiffRow {
                kind: DiffRowKind::Addition,
                name: addition.name.clone(),
                class: addition.class.clone(),
                properties,
                nested: count_descendants(&addition.children),
            });
        }
        for update in &pending.changes.updates {
            let existing = self
                .argon
                .ids
                .get(&update.id)
                .and_then(|&r| self.dom.get(r));
            let name = update
                .name
                .clone()
                .or_else(|| existing.map(|i| i.name().to_owned()))
                .unwrap_or_default();
            let class = update
                .class
                .clone()
                .or_else(|| existing.map(|i| i.class().to_owned()))
                .unwrap_or_default();
            let properties = update
                .properties
                .iter()
                .flatten()
                .filter_map(|(name, encoded)| {
                    let after = argon_client::decode_value(encoded)?;
                    let before = existing
                        .and_then(|i| i.properties().get(name))
                        .map(|value| format!("{value:?}"));
                    Some(PropertyDiff {
                        name: name.clone(),
                        before,
                        after: format!("{after:?}"),
                    })
                })
                .collect();
            rows.push(DiffRow {
                kind: DiffRowKind::Update,
                name,
                class,
                properties,
                nested: 0,
            });
        }
        for &id in &pending.changes.removals {
            let existing = self.argon.ids.get(&id).and_then(|&r| self.dom.get(r));
            let (name, class) = existing
                .map(|i| (i.name().to_owned(), i.class().to_owned()))
                .unwrap_or_default();
            rows.push(DiffRow {
                kind: DiffRowKind::Removal,
                name,
                class,
                properties: Vec::new(),
                nested: 0,
            });
        }
        rows
    }

    /// `RBX_STUDIO_ARGON_DIFF=1` opens the Diff window on a small synthetic
    /// review built out of whatever the DOM already holds — a real one only
    /// exists after a live `argon serve` session pushes a batch of five or
    /// more changes at once, which nothing else can arrange on the editor's
    /// behalf, the same reason every other `RBX_STUDIO_*` var exists.
    pub(super) fn apply_debug_argon_diff(&mut self, cx: &mut Context<Self>) {
        if std::env::var(DIFF_VARIABLE).as_deref() != Ok("1") {
            return;
        }
        self.seed_debug_diff_pending();
        self.open_argon_diff(cx);
    }

    /// Builds a `PendingReview` shaped like a real one — one addition (with
    /// a nested child, to exercise the "+N nested" row), and, against
    /// whatever this DOM's first two root services happen to be, one update
    /// (a rename plus a property change) and one removal.
    fn seed_debug_diff_pending(&mut self) {
        let addition = argon_client::Snapshot {
            id: ArgonRef::generate(),
            parent: None,
            name: "DebugPart".to_owned(),
            class: "Part".to_owned(),
            properties: Vec::new(),
            children: vec![argon_client::Snapshot {
                id: ArgonRef::generate(),
                parent: None,
                name: "Nested".to_owned(),
                class: "Part".to_owned(),
                properties: Vec::new(),
                children: Vec::new(),
            }],
        };

        let roots: Vec<Ref> = self.dom.root_refs().iter().copied().take(2).collect();
        let mut updates = Vec::new();
        let mut removals = Vec::new();
        for (index, referent) in roots.into_iter().enumerate() {
            let id = ArgonRef::generate();
            self.argon.ids.insert(id, referent);
            self.argon.ids_rev.insert(referent, id);
            if index == 0 {
                let transparency = argon_client::encode_value(&rbx_dom::Variant::Float32(0.5));
                updates.push(argon_client::UpdatedSnapshot {
                    id,
                    name: Some("Renamed".to_owned()),
                    class: None,
                    properties: transparency.map(|value| vec![("Transparency".to_owned(), value)]),
                });
            } else {
                removals.push(id);
            }
        }

        let changes = argon_client::Changes {
            additions: vec![addition],
            updates,
            removals,
        };
        self.argon.pending = Some(PendingReview {
            additions: changes.additions.len(),
            updates: changes.updates.len(),
            removals: changes.removals.len(),
            changes,
        });
    }

    /// The review prompt's Diff button: opens the detail window
    /// (`shell::argon_diff_window`), or raises it if one is already open —
    /// there is only ever one review pending at a time, so a second window
    /// would just be the same rows twice.
    pub(super) fn open_argon_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.argon_diff {
            let _ = existing.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let shell = cx.entity();
        // Deferred for the same reason `open_sequence_editor` defers: this
        // runs inside the click handler's own `Shell` update, and opening a
        // window renders it immediately — reading the entity that update is
        // still holding is a panic, not something the compiler catches.
        cx.defer(move |cx| {
            let opened = super::argon_diff_window::ArgonDiffWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.argon_diff = opened);
        });
    }

    /// `RBX_STUDIO_ARGON_CONNECT`: documented at [`CONNECT_VARIABLE`].
    pub(super) fn apply_debug_argon_connect(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(value) = std::env::var(CONNECT_VARIABLE) else {
            return;
        };
        if value != "1" {
            self.argon_address.update(cx, |state, cx| {
                state.set_value(value, window, cx);
            });
        }
        self.argon_connect(cx);
    }

    /// The dock's Connect button: reads the address field, opens the
    /// connection, and starts the poll loop that drains it.
    /// The plugin connects on its own when a place opens if `AutoConnect`
    /// is on (`argon-roblox@30fd38d:src/App/init.luau:120-125`). The
    /// `RBX_STUDIO_ARGON_CONNECT` aid wins when it is set, so a scripted
    /// screenshot gets exactly the address it asked for.
    pub(super) fn apply_argon_auto_connect(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if std::env::var_os(CONNECT_VARIABLE).is_some() {
            self.apply_debug_argon_connect(window, cx);
            return;
        }
        let keys = self.argon_level_keys();
        if self.argon_settings.get(Setting::AutoConnect, &keys) == Value::Bool(true) {
            self.argon_connect(cx);
        }
    }

    /// The Game and Place identities Argon's settings resolve against:
    /// the connected project's, or none while disconnected (the plugin
    /// keys them on the place's own IDs, `Config.luau:61-62`, which a local
    /// file doesn't have — see `settings::argon`).
    pub(super) fn argon_level_keys(&self) -> LevelKeys {
        match &self.argon.state {
            SyncState::Connected { keys, .. } => keys.clone(),
            _ => LevelKeys::default(),
        }
    }

    pub(super) fn argon_connect(&mut self, cx: &mut Context<Self>) {
        let address = self.argon_address.read(cx).value().to_string();
        let (host, port) = parse_address(&address);
        self.argon.generation = self.argon.generation.wrapping_add(1);
        let generation = self.argon.generation;
        self.argon.client = Some(ArgonClient::connect(host, port));
        self.argon.state = SyncState::Connecting;
        self.argon.pending = None;
        self.spawn_argon_poll(generation, cx);
        cx.notify();
    }

    /// The dock's Disconnect button. Dropping the client stops the
    /// background thread (see `ArgonClient::drop`); bumping `generation`
    /// stops the poll loop from scheduling another tick once it notices.
    pub(super) fn argon_disconnect(&mut self, cx: &mut Context<Self>) {
        self.argon.generation = self.argon.generation.wrapping_add(1);
        self.argon = Sync::default();
        cx.notify();
    }

    pub(super) fn argon_accept_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.argon.pending.take() {
            self.apply_argon_changes(pending.changes, cx);
            self.touch_last_sync(SyncDirection::Down, cx);
        }
    }

    pub(super) fn argon_cancel_pending(&mut self, cx: &mut Context<Self>) {
        self.argon.pending = None;
        cx.notify();
    }

    fn spawn_argon_poll(&self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let alive = shell
                .update(cx, |shell, cx| {
                    if shell.argon.generation != generation {
                        return false;
                    }
                    shell.drain_argon_events(cx);
                    true
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn drain_argon_events(&mut self, cx: &mut Context<Self>) {
        if self.argon.client.is_none() {
            return;
        }
        let events = self.argon.client.as_ref().unwrap().poll();
        if events.is_empty() {
            // Nothing new — still notify, so a live "Ns ago" readout keeps
            // counting up while connected.
            if matches!(self.argon.state, SyncState::Connected { .. }) {
                cx.notify();
            }
            return;
        }
        for event in events {
            match event {
                ArgonEvent::Connected(project) => {
                    let address = self.argon_address.read(cx).value().to_string();
                    let last_sync = match &self.argon.state {
                        SyncState::Connected { last_sync, .. } => *last_sync,
                        _ => None,
                    };
                    // Persisted here, not on every keystroke of a draft
                    // still being typed — this is the address that just
                    // actually worked.
                    if self.argon_saved_address != address {
                        self.argon_saved_address = address.clone();
                        self.save_settings();
                    }
                    self.argon.state = SyncState::Connected {
                        keys: level_keys(&project),
                        project: project.name,
                        address,
                        last_sync,
                        direction: SyncDirection::Down,
                    };
                }
                ArgonEvent::InitialSnapshot(snapshot) => {
                    self.apply_initial_snapshot(snapshot, cx);
                    self.touch_last_sync(SyncDirection::Down, cx);
                }
                ArgonEvent::Changes(changes) => self.handle_incoming_changes(changes, cx),
                ArgonEvent::Error(message) => {
                    self.argon.state = SyncState::Error(message);
                    self.argon.client = None;
                }
                ArgonEvent::Disconnected => {
                    if !matches!(self.argon.state, SyncState::Error(_)) {
                        self.argon.state = SyncState::NotConnected;
                    }
                    self.argon.client = None;
                }
            }
        }
        cx.notify();
    }

    fn touch_last_sync(&mut self, direction: SyncDirection, cx: &mut Context<Self>) {
        if let SyncState::Connected {
            last_sync,
            direction: current,
            ..
        } = &mut self.argon.state
        {
            *last_sync = Some(Instant::now());
            *current = direction;
        }
        cx.notify();
    }

    fn handle_incoming_changes(&mut self, changes: argon_client::Changes, cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        if changes.len() >= REVIEW_THRESHOLD {
            self.argon.pending = Some(PendingReview {
                additions: changes.additions.len(),
                updates: changes.updates.len(),
                removals: changes.removals.len(),
                changes,
            });
            cx.notify();
            return;
        }
        self.apply_argon_changes(changes, cx);
        self.touch_last_sync(SyncDirection::Down, cx);
    }

    // ---------------------------------------------------------- apply path

    fn apply_initial_snapshot(&mut self, snapshot: argon_client::Snapshot, cx: &mut Context<Self>) {
        self.push_history();
        self.argon.applying = true;
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for child in snapshot.children {
            self.apply_snapshot_node(&mut dom, child, None);
        }
        self.dom = dom;
        let log = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&log, cx);
        self.argon.applying = false;
        self.record_history_change(log);
        cx.notify();
    }

    fn apply_argon_changes(&mut self, changes: argon_client::Changes, cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        self.push_history();
        self.argon.applying = true;
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for addition in changes.additions {
            self.apply_addition(&mut dom, addition);
        }
        for update in changes.updates {
            self.apply_update(&mut dom, update);
        }
        for removal in changes.removals {
            self.apply_removal(&mut dom, removal);
        }
        self.dom = dom;
        let log = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&log, cx);
        self.argon.applying = false;
        self.record_history_change(log);
        cx.notify();
    }

    /// One `additions` entry: resolves its `parent` id against what this
    /// client already knows, and drops the entry (rather than guessing) if
    /// that parent hasn't been seen yet — a later full resync (or the
    /// parent's own addition, if it's in the same batch and ordered first,
    /// which `argon-roblox` itself doesn't guarantee) will bring it in.
    fn apply_addition(&mut self, dom: &mut WeakDom, node: argon_client::Snapshot) {
        match node.parent {
            Some(ArgonRef::ROOT) | None => {
                self.apply_snapshot_node(dom, node, None);
            }
            Some(parent_id) => {
                if let Some(&parent) = self.argon.ids.get(&parent_id) {
                    self.apply_snapshot_node(dom, node, Some(parent));
                }
            }
        }
    }

    /// One instance, recursively. With no local parent given, this is a
    /// root-level entry — reused against an existing service of the same
    /// class rather than duplicated, the same special case
    /// `argon-roblox`'s own `Processor.Write` makes for `game:
    /// FindFirstChildOfClass`.
    fn apply_snapshot_node(
        &mut self,
        dom: &mut WeakDom,
        node: argon_client::Snapshot,
        parent: Option<Ref>,
    ) -> Ref {
        let existing = parent.is_none().then(|| {
            dom.root_refs()
                .iter()
                .copied()
                .find(|&r| dom.get(r).is_some_and(|i| i.class() == node.class))
        });
        let referent = match existing.flatten() {
            Some(referent) => {
                if dom.get(referent).is_some_and(|i| i.name() != node.name) {
                    let _ = dom.set_name(referent, &node.name);
                }
                referent
            }
            None => dom.new_instance(&node.class, &node.name, parent),
        };
        self.argon.ids.insert(node.id, referent);
        self.argon.ids_rev.insert(referent, node.id);
        for (name, encoded) in &node.properties {
            if let Some(variant) = argon_client::decode_value(encoded) {
                let _ = dom.set_property(referent, name, variant);
            }
        }
        for child in node.children {
            self.apply_snapshot_node(dom, child, Some(referent));
        }
        referent
    }

    fn apply_update(&mut self, dom: &mut WeakDom, update: argon_client::UpdatedSnapshot) {
        let Some(&referent) = self.argon.ids.get(&update.id) else {
            return;
        };
        if dom.get(referent).is_none() {
            return;
        }
        if let Some(name) = &update.name {
            let _ = dom.set_name(referent, name);
        }
        if let Some(class) = &update.class {
            let _ = dom.set_class(referent, class);
        }
        // v1 simplification: properties present in the update are written;
        // ones the update omits are left as they stand rather than reset
        // to their class default (which `argon-roblox`'s own non-initial
        // sync path does) — see the Argon sync plan's known simplifications.
        if let Some(properties) = &update.properties {
            for (name, encoded) in properties {
                if let Some(variant) = argon_client::decode_value(encoded) {
                    let _ = dom.set_property(referent, name, variant);
                }
            }
        }
    }

    fn apply_removal(&mut self, dom: &mut WeakDom, id: ArgonRef) {
        let Some(&referent) = self.argon.ids.get(&id) else {
            return;
        };
        for removed in dom.remove(referent) {
            if let Some(removed_id) = self.argon.ids_rev.remove(&removed) {
                self.argon.ids.remove(&removed_id);
            }
        }
    }

    // ------------------------------------------------------- write-back

    /// `Shell::reflect_changes`'s own hook (called at the end of that
    /// function): notes which local instances a genuine edit just touched,
    /// then schedules a debounced `POST write` — unless this same batch is
    /// a remote one being applied, see this module's doc comment.
    pub(super) fn forward_to_argon(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        if self.argon.client.is_none() || self.argon.applying || changes.is_empty() {
            return;
        }
        for change in changes {
            match *change {
                Change::Added(referent)
                | Change::Property { referent, .. }
                | Change::Class(referent) => {
                    self.argon.dirty.insert(referent);
                }
                Change::Removed(referent) => {
                    self.argon.dirty.remove(&referent);
                    if let Some(id) = self.argon.ids_rev.remove(&referent) {
                        self.argon.ids.remove(&id);
                        self.argon.removed.push(id);
                    }
                }
                // Argon's `UpdatedSnapshot` has no parent field — a
                // reparent made locally isn't representable as an update
                // on this protocol, so it isn't forwarded. Any property,
                // name or class edit on the same instance still syncs.
                Change::Parent { .. } => {}
            }
        }
        self.schedule_argon_write(cx);
    }

    fn schedule_argon_write(&mut self, cx: &mut Context<Self>) {
        self.argon.write_generation = self.argon.write_generation.wrapping_add(1);
        let generation = self.argon.write_generation;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(WRITE_DEBOUNCE).await;
            let _ = shell.update(cx, |shell, cx| shell.flush_argon_write(generation, cx));
        })
        .detach();
    }

    fn flush_argon_write(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.argon.write_generation {
            return;
        }
        let removals = std::mem::take(&mut self.argon.removed);
        let dirty = ordered_parent_first(&self.dom, std::mem::take(&mut self.argon.dirty));
        let Some(client) = &self.argon.client else {
            return;
        };
        let mut additions = Vec::new();
        let mut updates = Vec::new();
        for referent in dirty {
            let Some(instance) = self.dom.get(referent) else {
                continue;
            };
            let properties: Vec<(String, rmpv::Value)> = instance
                .properties()
                .iter()
                .filter_map(|(name, variant)| {
                    argon_client::encode_value(variant).map(|value| (name.clone(), value))
                })
                .collect();
            match self.argon.ids_rev.get(&referent).copied() {
                Some(id) => updates.push(argon_client::UpdatedSnapshot {
                    id,
                    name: Some(instance.name().to_owned()),
                    class: Some(instance.class().to_owned()),
                    properties: Some(properties),
                }),
                None => {
                    let id = ArgonRef::generate();
                    self.argon.ids.insert(id, referent);
                    self.argon.ids_rev.insert(referent, id);
                    let parent = self
                        .dom
                        .parent(referent)
                        .and_then(|p| self.argon.ids_rev.get(&p).copied())
                        .unwrap_or(ArgonRef::ROOT);
                    additions.push(argon_client::Snapshot {
                        id,
                        parent: Some(parent),
                        name: instance.name().to_owned(),
                        class: instance.class().to_owned(),
                        properties,
                        children: Vec::new(),
                    });
                }
            }
        }
        let changes = argon_client::Changes {
            additions,
            updates,
            removals,
        };
        if !changes.is_empty() {
            client.write(changes);
            self.touch_last_sync(SyncDirection::Up, cx);
        }
    }
}

/// Orders a dirty set so a referent is only emitted once its parent is
/// either already known outside this batch, or already placed earlier in
/// the same batch — a package install (or any multi-instance edit) dirties
/// a parent `Folder`/`ModuleScript` alongside its own children in one go,
/// and `flush_argon_write` needs the parent's `ArgonRef` to exist before it
/// can name it as a child's parent. Draining the `HashSet` directly (as
/// this used to) processes referents in arbitrary hash order, so a child
/// could be visited before its own not-yet-assigned parent and silently
/// fall back to [`ArgonRef::ROOT`] — the wrong place on the Argon side.
///
/// Bounded by the batch's own depth (a handful of iterations for anything
/// this editor would realistically dirty in one edit); a `retain` pass
/// that places nothing at all — which a real tree can't produce, since a
/// root-level referent is always immediately ready — is treated as a
/// malformed edge case rather than looped on forever, and whatever's left
/// is appended in whatever order it was in.
fn ordered_parent_first(dom: &WeakDom, dirty: HashSet<Ref>) -> Vec<Ref> {
    let mut remaining: Vec<Ref> = dirty.iter().copied().collect();
    let mut placed: HashSet<Ref> = HashSet::new();
    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let before = ordered.len();
        remaining.retain(|&referent| {
            let ready = match dom.parent(referent) {
                Some(parent) => !dirty.contains(&parent) || placed.contains(&parent),
                None => true,
            };
            if ready {
                ordered.push(referent);
                placed.insert(referent);
            }
            !ready
        });
        if ordered.len() == before {
            ordered.append(&mut remaining);
            break;
        }
    }
    ordered
}

/// One addition's own subtree, minus its own root — what [`Shell::argon_diff_rows`]
/// reports as a row's "+N nested" count.
fn count_descendants(children: &[argon_client::Snapshot]) -> usize {
    children
        .iter()
        .map(|child| 1 + count_descendants(&child.children))
        .sum()
}

/// `"host:port"` (Argon's own default `localhost:8000`) → its two halves,
/// tolerant of a missing port (falls back to Argon's own default) or a
/// malformed one.
/// The settings levels a connected project identifies (option (b) of
/// #0022, the owner's pick): Game is the project's `game_id`, Place is its
/// place ID when the project has exactly one — with several there is no
/// telling which one this file is, so Place stays unidentified.
fn level_keys(project: &argon_client::Project) -> LevelKeys {
    LevelKeys {
        game: project.game_id.map(|id| id.to_string()),
        place: match project.place_ids.as_slice() {
            [id] => Some(id.to_string()),
            _ => None,
        },
    }
}

fn parse_address(address: &str) -> (String, u16) {
    match address.split_once(':') {
        Some((host, port)) => (host.trim().to_owned(), port.trim().parse().unwrap_or(8000)),
        None => (address.trim().to_owned(), 8000),
    }
}

#[cfg(test)]
mod tests;
