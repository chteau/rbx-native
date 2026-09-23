//! The connection half of `shell::argon_sync`: connecting to and
//! disconnecting from `argon serve`, the poll loop that drains the client
//! thread's events into `SyncState`, and what a connected project
//! identifies for the settings levels.

use std::time::{Duration, Instant};

use gpui_kit::Context;
use rbx_dom::Ref;

use crate::command_bar::Feedback;

use crate::argon_client::{self, ArgonClient, ArgonEvent};
use crate::settings::argon::{LevelKeys, Setting, Value};

use super::initial::{Priority, Rules};
use super::{Shell, Sync, SyncDirection, SyncState, CONNECT_VARIABLE, POLL_INTERVAL};

/// How long Auto Reconnect waits after a failed connection before trying
/// again (`argon-roblox@30fd38d:src/App/init.luau:59`).
const RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

/// The Output dock's source column for everything Argon writes.
const LOG_SOURCE: &str = "argon";

/// The plugin's log levels, in the order its `Log Level` setting ranks
/// them (`src/Log.luau:35-43`): a message shows when its level is at or
/// below the setting's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn parse(choice: &str) -> LogLevel {
        match choice {
            "Off" => LogLevel::Off,
            "Error" => LogLevel::Error,
            "Info" => LogLevel::Info,
            "Debug" => LogLevel::Debug,
            "Trace" => LogLevel::Trace,
            _ => LogLevel::Warn,
        }
    }
}

/// Whether a message at `level` gets through a `Log Level` of `setting`
/// (`Log.luau:56-84`: each writer compares its level to the current one).
pub(super) fn log_passes(setting: LogLevel, level: LogLevel) -> bool {
    level != LogLevel::Off && level <= setting
}

/// Whether a batch asks before it is applied: `Display Prompts` "Always",
/// "Initial" only for the initial sync, "Never" (`Core/init.luau:409-419`),
/// and then only when it exceeds `Changes Threshold` (`:233`, a strict
/// "more than").
pub(super) fn needs_review(
    display_prompts: &str,
    initial: bool,
    total: usize,
    threshold: u32,
) -> bool {
    let prompts = match display_prompts {
        "Always" => true,
        "Initial" => initial,
        _ => false,
    };
    prompts && total > threshold as usize
}

impl Shell {
    /// One line in the Output dock, if the `Log Level` setting lets it
    /// through. Errors and warnings land in their own Output filters;
    /// everything else is plain output.
    pub(super) fn argon_log(&mut self, level: LogLevel, message: impl Into<String>) {
        let keys = self.argon_level_keys();
        let setting = match self.argon_settings.get(Setting::LogLevel, &keys) {
            Value::Choice(choice) => LogLevel::parse(choice),
            _ => LogLevel::Warn,
        };
        if !log_passes(setting, level) {
            return;
        }
        let message = message.into();
        let feedback = match level {
            LogLevel::Error => Feedback::Error(message),
            LogLevel::Warn => Feedback::Warning(message),
            _ => Feedback::Output(message),
        };
        self.output.push(LOG_SOURCE, feedback);
    }

    /// Whether a script should open in the OS editor instead of the
    /// built-in one, and does so: the plugin's `OpenInEditor`
    /// (`Core/init.luau:357-395`) forwards a synced script to the server's
    /// `/open` and closes its own document. Not connected, setting off, or
    /// a script the server doesn't know: `false`, open it here.
    pub(in crate::shell) fn argon_open_in_editor(&self, referent: Ref) -> bool {
        if !matches!(self.argon.state, SyncState::Connected { .. }) {
            return false;
        }
        let keys = self.argon_level_keys();
        if self.argon_settings.get(Setting::OpenInEditor, &keys) != Value::Bool(true) {
            return false;
        }
        match (&self.argon.client, self.argon.ids_rev.get(&referent)) {
            (Some(client), Some(&id)) => {
                client.open(id, 1);
                true
            }
            _ => false,
        }
    }

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
            self.argon_ui.set_address(&value, window, cx);
        }
        self.argon_connect(cx);
    }

    /// The plugin connects on its own when a place opens if `AutoConnect`
    /// is on (`argon-roblox@30fd38d:src/App/init.luau:120-125`) — here
    /// only when the CLI is installed too, so a machine without Argon
    /// doesn't open every place to a failed connection. The
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
        if self.argon_settings.get(Setting::AutoConnect, &keys) != Value::Bool(true) {
            return;
        }
        // Without the CLI on this machine there is nothing to connect to
        // on its own; the Connect button stays, for a server elsewhere.
        if super::cli::locate().is_none() {
            self.argon_log(LogLevel::Info, "Argon CLI not found, Auto Connect skipped");
            return;
        }
        self.argon_connect(cx);
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
        let address = self.argon_ui.address(cx);
        let (host, port) = parse_address(&address);
        let keys = self.argon_level_keys();
        let https = self.argon_settings.get(Setting::Https, &keys) == Value::Bool(true);
        self.argon.generation = self.argon.generation.wrapping_add(1);
        let generation = self.argon.generation;
        self.argon_log(LogLevel::Info, format!("Connecting to {host}:{port}"));
        self.argon.client = Some(ArgonClient::connect(host, port, https));
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
                    let address = self.argon_ui.address(cx);
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
                    self.argon_log(LogLevel::Info, format!("Connected to {}", project.name));
                    self.argon.state = SyncState::Connected {
                        keys: level_keys(&project),
                        project: project.name,
                        last_sync,
                        direction: SyncDirection::Down,
                    };
                }
                ArgonEvent::InitialSnapshot(snapshot) => {
                    self.apply_initial_snapshot(snapshot, cx);
                    self.touch_last_sync(SyncDirection::Down, cx);
                }
                ArgonEvent::Changes(changes) => {
                    self.argon_log(
                        LogLevel::Debug,
                        format!("Received {} change(s) from the server", changes.len()),
                    );
                    self.handle_incoming_changes(changes, false, cx);
                }
                ArgonEvent::Error(message) => {
                    self.argon_log(LogLevel::Error, message.clone());
                    self.argon.state = SyncState::Error(message);
                    self.argon.client = None;
                    self.schedule_argon_reconnect(cx);
                }
                ArgonEvent::Disconnected => {
                    if !matches!(self.argon.state, SyncState::Error(_)) {
                        self.argon_log(LogLevel::Info, "Disconnected");
                        self.argon.state = SyncState::NotConnected;
                    }
                    self.argon.client = None;
                }
                ArgonEvent::OpenFailed(message) => {
                    self.argon_log(
                        LogLevel::Debug,
                        format!("Failed to open document in editor: {message}"),
                    );
                }
            }
        }
        cx.notify();
    }

    /// Auto Reconnect: five seconds after a failed connection, try again —
    /// unless something else touched the connection in the meantime
    /// (`App/init.luau:440-446` checks its `lastUpdate` the same way; the
    /// generation counter is this side's version of it).
    fn schedule_argon_reconnect(&mut self, cx: &mut Context<Self>) {
        let keys = self.argon_level_keys();
        if self.argon_settings.get(Setting::AutoReconnect, &keys) != Value::Bool(true) {
            return;
        }
        let generation = self.argon.generation;
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(RECONNECT_INTERVAL).await;
            let _ = shell.update(cx, |shell, cx| {
                if shell.argon.generation == generation
                    && matches!(shell.argon.state, SyncState::Error(_))
                {
                    shell.argon_log(LogLevel::Info, "Reconnecting");
                    shell.argon_connect(cx);
                }
            });
        })
        .detach();
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
