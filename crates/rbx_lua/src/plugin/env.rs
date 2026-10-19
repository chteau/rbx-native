//! State every plugin binding reaches through its VM.
//!
//! Each plugin VM carries one [`Env`] as mlua app data
//! (`lua.app_data_ref::<Rc<Env>>()`, see [`env`]); all VMs of a host share
//! one [`Shared`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use mlua::Lua;
use rbx_dom::Ref;

use super::api::{Context, Manifest, OutputKind, OutputLine, PluginId, Request, Ui, UiId};
use super::http::Http;
use crate::ctx::Ctx;
use crate::engine::{Engine, Host};
use crate::output::LineKind;

/// One plugin's VM-side state.
pub(crate) struct Env {
    pub(crate) plugin: PluginId,
    pub(crate) name: String,
    /// Bound to the host's shared DOM slot (see `PluginHost`).
    pub(crate) ctx: Ctx,
    pub(crate) shared: Rc<RefCell<Shared>>,
    /// This VM's threads, connections and loaded modules.
    pub(crate) engine: Rc<Engine>,
    /// Some for an rbx-native plugin (`plugin.toml`).
    pub(crate) manifest: Option<Manifest>,
    /// The plugin's own root instances (its script tree, its widgets, its
    /// `CoreGui` folder): parked between dispatches.
    pub(crate) roots: Rc<RefCell<Vec<Ref>>>,
    /// The plugin's `CoreGui` stand-in (a `Folder` named `CoreGui`, one per
    /// plugin, parked with its roots).
    pub(crate) core_gui: Ref,
    /// The plugin's `PluginGuiService` stand-in, where its widgets live.
    pub(crate) plugin_gui_service: Ref,
}

/// The [`Env`] of the VM running this callback.
///
/// Every plugin binding is only ever installed in a VM that has one, so a
/// missing `Env` is a host bug.
pub(crate) fn env(lua: &Lua) -> Rc<Env> {
    lua.app_data_ref::<Rc<Env>>()
        .expect("plugin binding called outside a plugin VM")
        .clone()
}

/// State shared by every plugin VM of one host.
#[derive(Default)]
pub(crate) struct Shared {
    /// Refreshed from the editor before every dispatch.
    pub(crate) context: Context,
    /// Set when a plugin wrote the selection during this dispatch.
    pub(crate) selection_written: bool,
    pub(crate) ui: Ui,
    pub(crate) ui_changed: bool,
    pub(crate) adornments_changed: bool,
    pub(crate) output: Vec<OutputLine>,
    pub(crate) requests: Vec<Request>,
    next_id: UiId,
    /// `plugin:GetSetting`/`SetSetting`, by plugin name; saved to
    /// `settings_dir/<name>.json` when written.
    pub(crate) settings: HashMap<String, serde_json::Map<String, serde_json::Value>>,
    pub(crate) settings_dir: Option<PathBuf>,
    pub(crate) http: Http,
    /// When the running dispatch must stop (see `vm::budget`).
    pub(crate) deadline: Option<Instant>,
    /// `PluginAction`s and toolbar buttons by id, to the plugin that owns them.
    pub(crate) owners: HashMap<UiId, PluginId>,
    /// Instances whose subtree a widget shows: a change under one bumps that
    /// widget's revision.
    pub(crate) widget_roots: HashSet<Ref>,
    /// The plugin `plugin:Activate` made current, and whether it took the
    /// mouse exclusively; one at a time, as in Studio.
    pub(crate) active: Option<(PluginId, bool)>,
    /// Plugins another one's `Activate` displaced: their `Deactivation`
    /// fires once the input is handled.
    pub(crate) displaced: Vec<PluginId>,
}

impl Shared {
    pub(crate) fn next_id(&mut self) -> UiId {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn push_output(&mut self, source: &str, kind: OutputKind, text: impl Into<String>) {
        self.output.push(OutputLine {
            source: source.to_string(),
            kind,
            text: text.into(),
        });
    }
}

/// What the shared engine asks of a plugin VM's host: time and ids come
/// from [`Shared`], output is the plugin's, and its parked roots are not
/// part of the place.
struct PluginThreads {
    name: String,
    shared: Rc<RefCell<Shared>>,
    roots: Rc<RefCell<Vec<Ref>>>,
}

impl Host for PluginThreads {
    fn now(&self) -> f64 {
        self.shared.borrow().context.now
    }

    fn set_now(&self, now: f64) {
        self.shared.borrow_mut().context.now = now;
    }

    fn next_id(&self) -> u32 {
        self.shared.borrow_mut().next_id()
    }

    fn output(&self, kind: LineKind, text: String) {
        let kind = match kind {
            LineKind::Print => OutputKind::Print,
            LineKind::Warn => OutputKind::Warn,
            LineKind::Error => OutputKind::Error,
        };
        self.shared.borrow_mut().push_output(&self.name, kind, text);
    }

    fn is_parked(&self, root: Ref) -> bool {
        self.roots.borrow().contains(&root)
    }
}

/// The engine for a plugin VM named `name` over `ctx`.
pub(crate) fn engine_for(
    name: &str,
    ctx: &Ctx,
    shared: &Rc<RefCell<Shared>>,
    roots: &Rc<RefCell<Vec<Ref>>>,
) -> Rc<Engine> {
    let host = PluginThreads {
        name: name.to_string(),
        shared: shared.clone(),
        roots: roots.clone(),
    };
    Engine::new(ctx.clone(), Rc::new(host))
}
