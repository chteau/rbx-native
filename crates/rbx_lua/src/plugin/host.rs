//! [`PluginHost`]: owns the plugin VMs and runs one input at a time against
//! the editor's DOM.

mod dispatch;
mod views;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::{Change, Instance, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::api::{Context, Input, Outcome, PluginId, PluginInfo, PluginSource, Ui, UiId};
use super::env::{Env, Shared};
use super::lot::Lot;
use crate::engine::scheduler;
use crate::engine::signal::{Owner, SignalKey};

pub(crate) struct Loaded {
    pub(crate) info: PluginInfo,
    pub(crate) source: Option<PluginSource>,
    pub(crate) lua: Option<Lua>,
    pub(crate) env: Option<Rc<Env>>,
}

pub struct PluginHost {
    pub(crate) shared: Rc<RefCell<Shared>>,
    /// The slot every VM's `Ctx` is bound to: the editor's DOM is swapped in
    /// for a dispatch and out again after it.
    pub(crate) dom: Rc<RefCell<WeakDom>>,
    pub(crate) database: Rc<ReflectionDatabase>,
    pub(crate) plugins: Vec<Loaded>,
    pub(crate) lot: Lot,
    pub(crate) last_tick: Option<f64>,
    /// See `Outcome::before`.
    pub(crate) undo: Rc<RefCell<super::undo::UndoPoint>>,
}

const FRAME_EVENTS: [&str; 3] = ["Heartbeat", "RenderStepped", "Stepped"];

impl PluginHost {
    /// `settings_dir`: where `plugin:SetSetting` persists (none = memory only).
    pub fn new(database: ReflectionDatabase, settings_dir: Option<PathBuf>) -> Self {
        let mut shared = Shared::default();
        shared.settings_dir = settings_dir;
        PluginHost {
            shared: Rc::new(RefCell::new(shared)),
            dom: Rc::new(RefCell::new(WeakDom::new())),
            database: Rc::new(database),
            plugins: Vec::new(),
            lot: Lot::default(),
            last_tick: None,
            undo: Rc::default(),
        }
    }

    /// Registers a plugin; it starts on the next [`Input::Start`].
    pub fn add(&mut self, source: PluginSource) -> PluginId {
        let id = PluginId::try_from(self.plugins.len() + 1).unwrap_or(PluginId::MAX);
        self.plugins.push(Loaded {
            info: PluginInfo {
                id,
                name: source.name.clone(),
                origin: source.origin.clone(),
                started: false,
                error: None,
            },
            source: Some(source),
            lua: None,
            env: None,
        });
        id
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.plugins.iter().map(|p| p.info.clone()).collect()
    }

    /// Runs one input. `dom` is the editor's place: it is borrowed for the
    /// call and handed back with the plugins' edits applied and their own
    /// instances parked again. Steps: swap `dom` into the slot, store
    /// `context`, merge the lot, set the deadline (5 s), handle `input`
    /// (see `Input`'s variants), then up to 8 rounds of
    /// `events::fire_changes` over the DOM's new changes across every VM,
    /// split the lot (bumping the revision of every widget whose subtree a
    /// change touched, and flagging adornments), swap back, and drain
    /// `Shared` into the `Outcome`.
    pub fn dispatch(&mut self, dom: &mut WeakDom, context: Context, input: Input) -> Outcome {
        dispatch::run(self, dom, context, input)
    }

    /// Whether `Input::Tick` at `now` would do anything: a due timer, a
    /// deferred thread, a finished or pending HTTP request, a `Heartbeat`/
    /// `RenderStepped`/`Stepped` connection.
    /// The plugin `plugin:Activate` made current, and whether it took the
    /// mouse exclusively; the editor hands it the 3D view's mouse
    /// (`Input::Mouse`) instead of selecting.
    pub fn active_plugin(&self) -> Option<(PluginId, bool)> {
        self.shared.borrow().active
    }

    pub fn wants_tick(&self, now: f64) -> bool {
        if self.shared.borrow().http.busy() {
            return true;
        }
        self.envs().any(|env| {
            let queue = env.engine.scheduler.borrow();
            // A timer not yet due still counts: the editor only keeps ticking
            // while this says so, so a sleeping `task.wait` would never wake.
            !queue.timers.is_empty() || scheduler::wants_tick(&queue, now) || {
                let signals = env.engine.signals.borrow();
                FRAME_EVENTS
                    .iter()
                    .any(|event| signals.has(&SignalKey::new(Owner::Service("RunService"), *event)))
            }
        })
    }

    /// Whether `Input::DomChanged(changes)` would reach any connection.
    pub fn watches(&self, changes: &[Change]) -> bool {
        let mut referents = Vec::new();
        for change in changes {
            match change {
                Change::Property { referent, .. }
                | Change::Class(referent)
                | Change::Added(referent)
                | Change::Removed(referent) => referents.push(*referent),
                Change::Parent { referent, old, new } => {
                    referents.push(*referent);
                    referents.extend(old.iter().chain(new.iter()).copied());
                }
            }
        }
        self.envs().any(|env| {
            let signals = env.engine.signals.borrow();
            // Descendant signals fire on ancestors this method cannot see
            // (the place is not in the slot between dispatches).
            let any_descendant = signals.connections.iter().any(|(key, list)| {
                !list.is_empty()
                    && matches!(key.owner, Owner::Instance(_))
                    && key.event.starts_with("Descendant")
            });
            any_descendant || referents.iter().any(|r| signals.watches_instance(*r))
        })
    }

    pub fn ui(&self) -> Ui {
        self.shared.borrow().ui.clone()
    }

    /// A DOM to render one widget with `rbx_viewer`: a `ScreenGui` carrying
    /// the widget root's referent, holding copies of the root's children
    /// with their referents unchanged (so hit boxes map back).
    pub fn widget_dom(&self, widget: UiId) -> Option<WeakDom> {
        views::widget_dom(self, widget)
    }

    /// Copies of every parked `GuiBase3d` instance (`Handles`,
    /// `ArcHandles`, `*HandleAdornment`, `SelectionBox`, …) whose `Adornee`
    /// is set, without children and with their referents, for the 3D view.
    pub fn adornments(&self) -> Vec<Instance> {
        views::adornments(self)
    }

    fn envs(&self) -> impl Iterator<Item = &Rc<Env>> {
        self.plugins.iter().filter_map(|p| p.env.as_ref())
    }
}
