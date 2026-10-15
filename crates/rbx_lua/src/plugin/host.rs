//! [`PluginHost`]: owns the plugin VMs and runs one input at a time against
//! the editor's DOM.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::{Change, Instance, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::api::{Context, Input, Outcome, PluginId, PluginInfo, PluginSource, Ui, UiId};
use super::env::{Env, Shared};
use super::lot::Lot;

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
}

impl PluginHost {
    /// `settings_dir`: where `plugin:SetSetting` persists (none = memory only).
    pub fn new(database: ReflectionDatabase, settings_dir: Option<PathBuf>) -> Self {
        todo!()
    }

    /// Registers a plugin; it starts on the next [`Input::Start`].
    pub fn add(&mut self, source: PluginSource) -> PluginId {
        todo!()
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        todo!()
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
        todo!()
    }

    /// Whether `Input::Tick` at `now` would do anything: a due timer, a
    /// deferred thread, a finished or pending HTTP request, a `Heartbeat`/
    /// `RenderStepped`/`Stepped` connection.
    pub fn wants_tick(&self, now: f64) -> bool {
        todo!()
    }

    /// Whether `Input::DomChanged(changes)` would reach any connection.
    pub fn watches(&self, changes: &[Change]) -> bool {
        todo!()
    }

    pub fn ui(&self) -> Ui {
        todo!()
    }

    /// A DOM to render one widget with `rbx_viewer`: a `ScreenGui` carrying
    /// the widget root's referent, holding copies of the root's children
    /// with their referents unchanged (so hit boxes map back).
    pub fn widget_dom(&self, widget: UiId) -> Option<WeakDom> {
        todo!()
    }

    /// Copies of every parked `GuiBase3d` instance (`Handles`,
    /// `ArcHandles`, `*HandleAdornment`, `SelectionBox`, …) whose `Adornee`
    /// is set, without children and with their referents, for the 3D view.
    pub fn adornments(&self) -> Vec<Instance> {
        todo!()
    }
}
