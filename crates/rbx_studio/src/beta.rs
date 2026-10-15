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

use std::collections::BTreeSet;
use std::path::Path;

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
pub(crate) const FEATURES: &[Feature] = &[];

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
        todo!()
    }

    pub(crate) fn to_json(&self) -> String {
        todo!()
    }

    /// A missing or unreadable file is no choices.
    pub(crate) fn load(path: &Path) -> Choices {
        todo!()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        todo!()
    }

    pub(crate) fn is_on(&self, id: &str) -> bool {
        todo!()
    }

    pub(crate) fn set(&mut self, id: &str, on: bool) {
        todo!()
    }

    /// Ids of `listed` whose state differs between `self` and `other`.
    pub(crate) fn differences(&self, other: &Choices, listed: &[Feature]) -> Vec<&'static str> {
        todo!()
    }
}

/// [`FEATURES`], plus [`SAMPLE`] when `RBX_STUDIO_BETA_SAMPLE` is set.
pub(crate) fn features() -> Vec<Feature> {
    todo!()
}

/// Reads `beta.json` once, at launch; later calls do nothing. Before it
/// runs, every feature is off.
pub(crate) fn init() {
    todo!()
}

/// Whether `id` is on for this run (the choice at launch).
pub(crate) fn enabled(id: &str) -> bool {
    todo!()
}

/// The stored choice, which may differ from [`enabled`] until a restart.
pub(crate) fn chosen(id: &str) -> bool {
    todo!()
}

/// Stores a choice; it applies at the next launch.
pub(crate) fn choose(id: &str, on: bool) -> std::io::Result<()> {
    todo!()
}

/// How many listed features a restart would switch.
pub(crate) fn pending() -> usize {
    todo!()
}

#[cfg(test)]
mod tests;
