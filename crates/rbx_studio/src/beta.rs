//! Beta features: real Studio's File › Beta Features. A half-finished
//! feature ships switched off and asks [`enabled`] before it shows itself;
//! the user switches it on in Settings › Beta features.
//!
//! A switch applies after a restart, as in Studio: [`enabled`] answers from
//! the choices as they stood at launch ([`init`]), while the Settings page
//! edits the stored choices (`<config>/beta.json`,
//! `{"enabled": ["<id>", …]}`) and counts what a restart would change.
//!
//! Adding one: a [`Feature`] in [`FEATURES`], then `beta::enabled("<id>")`
//! at the place the feature appears. Remove both once it graduates.

use crate::settings::default_config_dir;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LAUNCH: OnceLock<Choices> = OnceLock::new();
static STORED: Mutex<Option<Choices>> = Mutex::new(None);

fn stored_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("beta.json"))
}

fn with_stored<T>(f: impl FnOnce(&mut Choices) -> T) -> T {
    let mut guard = STORED.lock().unwrap_or_else(|e| e.into_inner());
    let choices = guard.get_or_insert_with(|| match stored_path() {
        Some(path) => Choices::load(&path),
        None => Choices::default(),
    });
    f(choices)
}

/// One switchable feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Feature {
    /// The stable name `beta.json` stores.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// One or two sentences: what it does and what is unfinished.
    pub(crate) description: &'static str,
}

/// What ships behind a switch today.
pub(crate) const FEATURES: &[Feature] = &[
    Feature {
        id: crate::shell::COEDIT_BETA_ID,
        name: "Live co-editing",
        description: "Shares edits live with others in the place's Team Presence room (hosted relay only). Remote edits are undo steps of their own; concurrent script edits are merged, with conflicts kept side by side.",
    },
    Feature {
        id: crate::update_planning::BETA_ID,
        name: "Update planning",
        description: "Countdown icons on autopilot: timers over the current icon, uploaded for review early, swapped in at their time by the editor or a background helper the OS runs while the PC is on. Not yet tried against Roblox: the icon swap needs the key's legacy-universe:manage permission.",
    },
    Feature {
        id: crate::shell::MCP_BETA_ID,
        name: "MCP server",
        description: "Lets an AI client drive the open place over MCP: point the client at `rbxstudio --mcp`. It can read and edit scripts, run Luau, start and stop Play, read the Output, capture the viewport, simulate player input and insert assets. Several open editors are told apart by studio ID. Edits are undo steps of their own; `http_get` and `skill` serve Roblox documentation and best-practice guides.",
    },
    Feature {
        id: crate::shell::PERF_BETA_ID,
        name: "Performance heatmap: totals and models",
        description: "Adds Total and Physics to the Viewport dock's Performance view and ranks the costliest models with a breakdown of what each costs: triangles, texture memory, instances, physics, shadows and overdraw. The weights are an estimate, not Roblox's own costs.",
    },
    Feature {
        id: crate::shell::REQUIRE_GRAPH_BETA_ID,
        name: "Module require graph",
        description: "View › Require Graph draws every script and ModuleScript with an edge for each require it can resolve statically, and lists cycles, client scripts requiring server-only modules, requires it can't follow, and modules nothing requires.",
    },
];

/// `RBX_STUDIO_BETA_SAMPLE=1` lists this one too, so the page can be
/// exercised (and captured) while [`FEATURES`] is empty. It gates nothing.
pub(crate) const SAMPLE: Feature = Feature {
    id: "sample",
    name: "Sample feature",
    description: "Gates nothing. Listed only when RBX_STUDIO_BETA_SAMPLE is set, to try the page.",
};

/// The stored choices. Pure; the app's copies are behind the functions
/// below.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Choices {
    pub(crate) enabled: BTreeSet<String>,
}

impl Choices {
    /// Malformed text is no choices.
    pub(crate) fn from_json(text: &str) -> Choices {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Choices::default();
        };
        let enabled = value
            .get("enabled")
            .and_then(serde_json::Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Choices { enabled }
    }

    pub(crate) fn to_json(&self) -> String {
        let ids: Vec<&str> = self.enabled.iter().map(String::as_str).collect();
        serde_json::json!({ "enabled": ids }).to_string()
    }

    /// A missing or unreadable file is no choices.
    pub(crate) fn load(path: &Path) -> Choices {
        std::fs::read_to_string(path)
            .map(|text| Choices::from_json(&text))
            .unwrap_or_default()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, path)
    }

    pub(crate) fn is_on(&self, id: &str) -> bool {
        self.enabled.contains(id)
    }

    pub(crate) fn set(&mut self, id: &str, on: bool) {
        if on {
            self.enabled.insert(id.to_owned());
        } else {
            self.enabled.remove(id);
        }
    }

    /// Every listed feature back to its default, which is off. Ids stored
    /// for features that are no longer listed are left alone.
    pub(crate) fn reset(&mut self, listed: &[Feature]) {
        for feature in listed {
            self.set(feature.id, false);
        }
    }

    /// Ids of `listed` whose state differs between `self` and `other`.
    pub(crate) fn differences(&self, other: &Choices, listed: &[Feature]) -> Vec<&'static str> {
        listed
            .iter()
            .filter(|feature| self.is_on(feature.id) != other.is_on(feature.id))
            .map(|feature| feature.id)
            .collect()
    }
}

/// [`FEATURES`], plus [`SAMPLE`] when `RBX_STUDIO_BETA_SAMPLE` is set.
pub(crate) fn features() -> Vec<Feature> {
    let mut all = FEATURES.to_vec();
    if std::env::var("RBX_STUDIO_BETA_SAMPLE").is_ok_and(|value| !value.is_empty()) {
        all.push(SAMPLE);
    }
    all
}

/// Reads `beta.json` once, at launch; later calls do nothing. Before it
/// runs, every feature is off.
pub(crate) fn init() {
    let choices = stored_path()
        .map(|path| Choices::load(&path))
        .unwrap_or_default();
    let _ = LAUNCH.set(choices);
}

/// Whether `id` is on for this run (the choice at launch).
pub(crate) fn enabled(id: &str) -> bool {
    LAUNCH.get().is_some_and(|choices| choices.is_on(id))
}

/// The stored choice, which may differ from [`enabled`] until a restart.
pub(crate) fn chosen(id: &str) -> bool {
    with_stored(|choices| choices.is_on(id))
}

/// Stores a choice; it applies at the next launch.
pub(crate) fn choose(id: &str, on: bool) -> std::io::Result<()> {
    let path =
        stored_path().ok_or_else(|| std::io::Error::other("no config directory for beta.json"))?;
    with_stored(|choices| {
        choices.set(id, on);
        choices.save(&path)
    })
}

/// Switches every listed feature back off, saved the way [`choose`] saves.
pub(crate) fn reset_all() -> std::io::Result<()> {
    let path =
        stored_path().ok_or_else(|| std::io::Error::other("no config directory for beta.json"))?;
    with_stored(|choices| {
        choices.reset(&features());
        choices.save(&path)
    })
}

/// How many listed features a restart would switch.
pub(crate) fn pending() -> usize {
    let launch = LAUNCH.get().cloned().unwrap_or_default();
    let stored = with_stored(|choices| choices.clone());
    launch.differences(&stored, &features()).len()
}

#[cfg(test)]
mod tests;
