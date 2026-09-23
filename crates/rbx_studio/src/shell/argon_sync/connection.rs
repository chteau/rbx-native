//! The connection half of `shell::argon_sync`: connecting to and
//! disconnecting from `argon serve`, the poll loop that drains the client
//! thread's events into `SyncState`, and what a connected project
//! identifies for the settings levels.

use std::time::Instant;

use gpui_kit::Context;

use crate::argon_client::{self, ArgonClient, ArgonEvent};
use crate::settings::argon::{LevelKeys, Setting, Value};

use super::initial::{Priority, Rules};
use super::{Shell, Sync, SyncDirection, SyncState, CONNECT_VARIABLE, POLL_INTERVAL};

impl Shell {
    /// `RBX_STUDIO_ARGON_CONNECT`: documented at [`CONNECT_VARIABLE`].
    pub(in crate::shell) fn apply_debug_argon_connect(
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
    pub(in crate::shell) fn apply_argon_auto_connect(
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
    pub(in crate::shell) fn argon_level_keys(&self) -> LevelKeys {
        match &self.argon.state {
            SyncState::Connected { keys, .. } => keys.clone(),
            _ => LevelKeys::default(),
        }
    }

    /// What the initial sync reads from the settings, resolved against the
    /// connected project's levels.
    pub(super) fn argon_rules(&self) -> Rules {
        let keys = self.argon_level_keys();
        let on = |setting| self.argon_settings.get(setting, &keys) == Value::Bool(true);
        Rules {
            priority: match self.argon_settings.get(Setting::InitialSyncPriority, &keys) {
                Value::Choice("Client") => Priority::Client,
                Value::Choice("None") => Priority::None,
                _ => Priority::Server,
            },
            keep_unknowns: on(Setting::KeepUnknowns),
            override_packages: on(Setting::OverridePackages),
            syncback_properties: on(Setting::SyncbackProperties),
        }
    }

    pub(in crate::shell) fn argon_connect(&mut self, cx: &mut Context<Self>) {
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
    pub(in crate::shell) fn argon_disconnect(&mut self, cx: &mut Context<Self>) {
        self.argon.generation = self.argon.generation.wrapping_add(1);
        self.argon = Sync::default();
        cx.notify();
    }

    pub(in crate::shell) fn argon_accept_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.argon.pending.take() {
            self.apply_argon_changes(pending.changes, cx);
            self.touch_last_sync(SyncDirection::Down, cx);
        }
    }

    pub(in crate::shell) fn argon_cancel_pending(&mut self, cx: &mut Context<Self>) {
        self.argon.pending = None;
        cx.notify();
    }

    pub(super) fn spawn_argon_poll(&self, generation: u64, cx: &mut Context<Self>) {
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

    pub(super) fn drain_argon_events(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn touch_last_sync(&mut self, direction: SyncDirection, cx: &mut Context<Self>) {
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
}

/// `"host:port"` (Argon's own default `localhost:8000`) → its two halves,
/// tolerant of a missing port (falls back to Argon's own default) or a
/// malformed one.
/// The settings levels a connected project identifies: Game is the
/// project's `game_id`, Place is its place ID when the project has exactly
/// one. With several places there is no telling which of them this file
/// is, so Place stays unidentified rather than guessed.
pub(super) fn level_keys(project: &argon_client::Project) -> LevelKeys {
    LevelKeys {
        game: project.game_id.map(|id| id.to_string()),
        place: match project.place_ids.as_slice() {
            [id] => Some(id.to_string()),
            _ => None,
        },
    }
}

pub(super) fn parse_address(address: &str) -> (String, u16) {
    match address.split_once(':') {
        Some((host, port)) => (host.trim().to_owned(), port.trim().parse().unwrap_or(8000)),
        None => (address.trim().to_owned(), 8000),
    }
}
