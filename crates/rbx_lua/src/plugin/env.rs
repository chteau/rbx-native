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

use super::api::{
    Context, Manifest, OutputKind, OutputLine, PluginId, PluginProfile, Request, Ui, UiId,
};
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
    /// Who the plugin is to the permission store.
    pub(crate) profile: PluginProfile,
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
    /// The editor's permission store, which every check reads.
    pub(crate) permissions: rbx_permissions::Shared,
    /// Script injection prompts waiting for an answer, by plugin.
    pub(crate) script_prompts: HashMap<PluginId, UiId>,
}

/// Where HTTP answers used to live in a plugin's settings.
const LEGACY_HTTP_KEY: &str = "__rbxnative_http";
/// Where dock widget states live in a plugin's settings.
const WIDGETS_KEY: &str = "__rbxnative_widgets";
/// Where floating dock widgets were left, in a plugin's settings.
const RECTS_KEY: &str = "__rbxnative_widget_rects";

impl Shared {
    fn settings_file(dir: &std::path::Path, name: &str) -> PathBuf {
        dir.join(format!("{}.json", name.replace(['/', '\\'], "_")))
    }

    /// Reads plugin `name`'s settings saved by an earlier session, once.
    pub(crate) fn load_settings(&mut self, name: &str) {
        let Some(dir) = &self.settings_dir else {
            return;
        };
        if self.settings.contains_key(name) {
            return;
        }
        let saved = std::fs::read_to_string(Self::settings_file(dir, name))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        if let Some(map) = saved {
            self.settings.insert(name.to_string(), map);
            self.move_http_answers(name);
        }
    }

    /// Earlier builds kept a Roblox plugin's HTTP answers in its own
    /// settings; they belong to the permission store now.
    fn move_http_answers(&mut self, name: &str) {
        let Some(answers) = self
            .settings
            .get_mut(name)
            .and_then(|map| map.remove(LEGACY_HTTP_KEY))
        else {
            return;
        };
        let hosts: Vec<(String, bool)> = answers
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(host, allow)| Some((host.clone(), allow.as_bool()?)))
            .collect();
        let saved = self.permissions.borrow_mut().change(|policy| {
            for (host, allow) in &hosts {
                if policy.stored_host(name, host).is_none() {
                    policy.set_host(name, host, Some(*allow));
                }
            }
        });
        match saved {
            Ok(()) => self.save_settings(name),
            Err(error) => self.push_output(name, OutputKind::Warn, format!("permissions: {error}")),
        }
    }

    /// Writes plugin `name`'s settings to `settings_dir/<name>.json`.
    pub(crate) fn save_settings(&mut self, name: &str) {
        let (Some(dir), Some(map)) = (self.settings_dir.clone(), self.settings.get(name)) else {
            return;
        };
        let file = Self::settings_file(&dir, name);
        let written = serde_json::to_string_pretty(map)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::write(&file, text))
                    .map_err(|e| e.to_string())
            });
        if let Err(error) = written {
            self.push_output(
                name,
                crate::plugin::api::OutputKind::Warn,
                format!("could not save settings: {error}"),
            );
        }
    }

    /// The `Enabled` a dock widget had when plugin `name` last ran, kept
    /// under `__rbxnative_widgets` in its settings: Studio restores it
    /// unless the widget's info sets `InitialEnabledShouldOverrideRestore`.
    pub(crate) fn restored_widget(&self, name: &str, key: &str) -> Option<bool> {
        self.settings
            .get(name)?
            .get(WIDGETS_KEY)?
            .get(key)?
            .as_bool()
    }

    pub(crate) fn widget_rect(&self, name: &str, key: &str) -> Option<[f32; 4]> {
        let rect = self
            .settings
            .get(name)?
            .get(RECTS_KEY)?
            .get(key)?
            .as_array()?;
        let n = |i: usize| rect.get(i)?.as_f64().map(|v| v as f32);
        Some([n(0)?, n(1)?, n(2)?, n(3)?])
    }

    pub(crate) fn remember_widget_rect(&mut self, name: &str, key: &str, rect: [f32; 4]) {
        if self.widget_rect(name, key) == Some(rect) {
            return;
        }
        let map = self.settings.entry(name.to_string()).or_default();
        let rects = map
            .entry(RECTS_KEY)
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let Some(rects) = rects.as_object_mut() {
            rects.insert(key.to_string(), serde_json::json!(rect));
        }
        self.save_settings(name);
    }

    /// Remembers a dock widget's `Enabled` for [`Shared::restored_widget`].
    pub(crate) fn remember_widget(&mut self, name: &str, key: &str, enabled: bool) {
        if self.restored_widget(name, key) == Some(enabled) {
            return;
        }
        let map = self.settings.entry(name.to_string()).or_default();
        let widgets = map
            .entry(WIDGETS_KEY)
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let Some(widgets) = widgets.as_object_mut() {
            widgets.insert(key.to_string(), serde_json::Value::Bool(enabled));
        }
        self.save_settings(name);
    }

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

#[cfg(test)]
mod tests {
    use super::Shared;

    #[test]
    fn old_http_answers_move_to_the_permission_store() {
        let dir = std::env::temp_dir().join(format!("rbx_env_http_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Tool.json");
        std::fs::write(
            &file,
            r#"{"__rbxnative_http": {"a.com": true, "b.com": false}, "kept": 1}"#,
        )
        .unwrap();
        let mut shared = Shared {
            settings_dir: Some(dir.clone()),
            ..Shared::default()
        };
        shared.load_settings("Tool");
        {
            let store = shared.permissions.borrow();
            assert_eq!(store.policy().stored_host("Tool", "a.com"), Some(true));
            assert_eq!(store.policy().stored_host("Tool", "b.com"), Some(false));
        }
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(!saved.contains("__rbxnative_http"));
        assert!(saved.contains("kept"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
