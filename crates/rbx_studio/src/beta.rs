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
pub(crate) const FEATURES: &[Feature] = &[Feature {
    id: crate::shell::COEDIT_BETA_ID,
    name: "Live co-editing",
    description: "Shares edits live with others in the place's Team Presence room (hosted relay only). Remote edits are undo steps of their own; concurrent script edits are merged, with conflicts kept side by side.",
}];

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

/// How many listed features a restart would switch.
pub(crate) fn pending() -> usize {
    let launch = LAUNCH.get().cloned().unwrap_or_default();
    let stored = with_stored(|choices| choices.clone());
    launch.differences(&stored, &features()).len()
}

#[cfg(test)]
mod tests;
