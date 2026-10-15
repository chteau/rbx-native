//! Plugins: real Roblox Studio plugins (`.lua`/`.rbxm`/`.rbxmx` from
//! rbx-native's plugins folder and, when found, Studio's own) run through
//! `rbx_lua::plugin::PluginHost`, and rbx-native's own plugins
//! (`plugin.toml` with a Luau or WASM entry; WASM runs under `rbx_wasm`).
//!
//! Every input reaches the host through [`Shell::plugin_input`], which lends
//! it `self.dom` for the call and applies the `Outcome`: the change log
//! (reflected like any other edit, one undo step for a user-initiated
//! input), the selection, Output lines, requests (undo/redo, prompts,
//! opening a script, debugger control) and UI changes.

mod debugger_bridge;
mod discover;
mod handles;
mod hit;
mod manager;
mod outcome;
mod overlays;
mod prompts;
mod render;
mod ribbon;
mod wasm;
mod widgets;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui_kit::*;
use rbx_dom::{Change, Ref};
use rbx_lua::plugin::{GuiEventKind, Input, PluginHost, Ui};

use super::Shell;
use crate::command_bar::Feedback;

const POLL_INTERVAL: Duration = Duration::from_millis(30);

/// Everything the editor keeps about plugins.
#[derive(Default)]
pub(crate) struct Plugins {
    /// `None` until [`Shell::plugins_start`] ran.
    pub(super) host: Option<PluginHost>,
    /// rbx-native WASM plugins.
    pub(super) wasm: Vec<wasm::Entry>,
    /// Every plugin file/folder found, enabled or not, for the manager.
    pub(super) found: Vec<discover::Found>,
    /// The host's UI as of its last change.
    pub(super) ui: Ui,
    /// The poll loop (`Input::Tick` while the host wants ticks).
    pub(super) poll: Option<Task<()>>,
    /// Set while an outcome is being applied, so the edits it reflects are
    /// not fed back to the plugins as `Input::DomChanged`.
    pub(super) applying: bool,
    /// The selection as the plugins last saw it.
    pub(super) last_selection: Vec<Ref>,
    pub(super) prompts: prompts::Prompts,
    pub(super) widgets: widgets::Widgets,
    pub(super) handles: handles::Handles,
    /// The part under the cursor in the viewport and the world point hit.
    pub(super) mouse: Option<(Ref, [f32; 3])>,
    /// The 3D view's bounds as last laid out, for `Camera.ViewportSize`.
    pub(super) view_bounds: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// The Manage Plugins dialog is open.
    pub(super) manager_open: bool,
    /// Origin of `Context::now`.
    pub(super) epoch: Option<Instant>,
    /// Plugin paths the user switched off (persisted in settings).
    pub(super) disabled: HashSet<String>,
}

impl Shell {
    /// Discovers the installed plugins (see `discover`), adds every enabled
    /// one to a fresh host (Luau/Roblox) or `wasm` list, dispatches
    /// `Input::Start`, starts the WASM ones, and starts the poll loop.
    /// Errors become Output warnings under the plugin's name; nothing here
    /// can fail the editor.
    pub(super) fn plugins_start(&mut self, cx: &mut Context<Self>) {
        self.plugins.epoch.get_or_insert_with(Instant::now);
        let prefs = manager::load_prefs();
        self.plugins.disabled = prefs.disabled.iter().cloned().collect();
        let mut found = Vec::new();
        if let Some(dir) = discover::own_dir() {
            found.extend(discover::scan(&dir, false));
        }
        if prefs.load_studio {
            if let Some(dir) = discover::studio_dir() {
                found.extend(discover::scan(&dir, true));
            }
        }
        let settings_dir =
            crate::settings::default_config_dir().map(|dir| dir.join("plugin-settings"));
        let mut host = PluginHost::new(self.database.clone(), settings_dir);
        let mut wasm = Vec::new();
        for plugin in &found {
            if self
                .plugins
                .disabled
                .contains(&*plugin.path.to_string_lossy())
            {
                continue;
            }
            let loaded = match plugin.kind {
                discover::Kind::Roblox | discover::Kind::NativeLuau => {
                    rbx_lua::plugin::read_source(&plugin.path).map(|source| {
                        host.add(source);
                    })
                }
                discover::Kind::NativeWasm => {
                    wasm::load(&plugin.path).map(|entry| wasm.push(entry))
                }
            };
            if let Err(message) = loaded {
                self.output
                    .push_plugin(&plugin.name, Feedback::Warning(message));
            }
        }
        self.plugins.found = found;
        self.plugins.wasm = wasm;
        self.plugins.host = Some(host);
        self.plugins.last_selection = self.selected_all().to_vec();
        self.plugin_input(Input::Start, cx);
        self.wasm_start(cx);
        self.plugins_arm_poll(cx);
        cx.notify();
    }

    /// Unloads everything (`Input::Unload` for each plugin) and starts again.
    pub(super) fn plugins_reload(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<_> = self
            .plugins
            .host
            .as_ref()
            .map(|host| host.plugins().into_iter().map(|p| p.id).collect())
            .unwrap_or_default();
        for id in ids {
            self.plugin_input(Input::Unload(id), cx);
        }
        self.plugins.poll = None;
        self.plugins.host = None;
        self.plugins.wasm.clear();
        self.plugins.prompts.queue.clear();
        self.plugins.ui = Ui::default();
        self.plugin_widgets_changed(cx);
        self.sync_plugin_handles(cx);
        self.plugins_start(cx);
    }

    /// Runs one input through the host. User-initiated inputs (button,
    /// action, prompt answer, widget GUI, handle, widget closed) are one
    /// undo step when they change the place: the DOM is cloned first and
    /// pushed with `push_history_snapshot` only if the outcome has
    /// changes. Does nothing during Play (`live_shown`) or before start.
    pub(super) fn plugin_input(&mut self, input: Input, cx: &mut Context<Self>) {
        if self.live_shown() || self.plugins.host.is_none() {
            return;
        }
        // Pointer motion over a widget is not an edit, and cloning the
        // place for it on every move would cost a full copy per event.
        let undoable = match &input {
            Input::Gui(event) => !matches!(
                event.kind,
                GuiEventKind::Enter { .. }
                    | GuiEventKind::Leave { .. }
                    | GuiEventKind::Move { .. }
                    | GuiEventKind::Scroll { .. }
            ),
            Input::ButtonClicked(_)
            | Input::ActionTriggered(_)
            | Input::PromptAnswered(..)
            | Input::Handle(_)
            | Input::WidgetClosed(_) => true,
            _ => false,
        };
        let context = self.plugin_context();
        // Everything logged so far was already reflected; the host reports
        // its own edits in the outcome.
        self.dom.take_changes();
        let before = undoable.then(|| self.dom.clone());
        let Some(host) = self.plugins.host.as_mut() else {
            return;
        };
        let outcome = host.dispatch(&mut self.dom, context, input);
        self.dom.take_changes();
        let snapshot = match before {
            Some(before) if !outcome.changes.is_empty() => {
                self.push_history_snapshot(before);
                true
            }
            _ => false,
        };
        self.apply_plugin_outcome(outcome, snapshot, cx);
        self.plugins_arm_poll(cx);
    }

    /// From `selection_changed`: `Input::SelectionChanged` when the
    /// selection differs from `last_selection`.
    pub(super) fn plugins_selection_changed(&mut self, cx: &mut Context<Self>) {
        if self.selected_all() == self.plugins.last_selection.as_slice() {
            return;
        }
        self.plugins.last_selection = self.selected_all().to_vec();
        self.plugin_input(Input::SelectionChanged, cx);
    }

    /// From `reflect_changes`: `Input::DomChanged` when not `applying` and
    /// `PluginHost::watches(changes)`.
    pub(super) fn plugins_dom_changed(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        if self.plugins.applying {
            return;
        }
        let watched = self
            .plugins
            .host
            .as_ref()
            .is_some_and(|host| host.watches(changes));
        if watched {
            self.plugin_input(Input::DomChanged(changes.to_vec()), cx);
        }
    }

    /// Painted at the window root: the open prompt, floating widgets and
    /// the Manage Plugins dialog.
    pub(super) fn plugin_overlays(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut overlays = Vec::new();
        overlays.extend(self.floating_widget_overlays(window, cx));
        overlays.extend(self.plugin_prompt_overlay(window, cx));
        overlays.extend(self.plugin_manager_overlay(window, cx));
        overlays
    }

    fn plugin_context(&self) -> rbx_lua::plugin::Context {
        rbx_lua::plugin::Context {
            now: self.plugins_now(),
            selection: self.selected_all().to_vec(),
            debugger: self.plugin_debugger_view(),
            mouse_target: self.plugins.mouse.map(|(part, _)| part),
            mouse_hit: self.plugins.mouse.map(|(_, hit)| hit),
            viewport_size: self
                .plugins
                .view_bounds
                .get()
                .map(|b| [f32::from(b.size.width), f32::from(b.size.height)]),
        }
    }

    fn plugins_now(&self) -> f64 {
        self.plugins
            .epoch
            .map_or(0.0, |epoch| epoch.elapsed().as_secs_f64())
    }

    /// Starts the tick loop when the host has something to tick and no loop
    /// runs; the loop ends itself once the host goes idle.
    fn plugins_arm_poll(&mut self, cx: &mut Context<Self>) {
        if self.plugins.poll.is_some() {
            return;
        }
        let now = self.plugins_now();
        if !self
            .plugins
            .host
            .as_ref()
            .is_some_and(|host| host.wants_tick(now))
        {
            return;
        }
        self.plugins.poll = Some(cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                match shell.update(cx, |shell, cx| shell.plugins_tick(cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
            let _ = shell.update(cx, |shell, _| shell.plugins.poll = None);
        }));
    }

    fn plugins_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.plugins.host.is_none() {
            return false;
        }
        self.plugin_input(Input::Tick, cx);
        let now = self.plugins_now();
        self.plugins
            .host
            .as_ref()
            .is_some_and(|host| host.wants_tick(now))
    }
}
