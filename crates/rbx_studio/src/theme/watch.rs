//! Noticing that the theme changed on disk — another theme picked in
//! `appearance.json`, or the active theme's own files edited — so the switch
//! happens live rather than on the next launch, and a theme author sees an
//! edit a second after saving it.
//!
//! Polls a fingerprint of file sizes and modification times rather than
//! subscribing to a platform file watcher: two small directories once a
//! second costs nothing, and needs no new dependency or per-OS backend.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::settings::default_config_dir;

pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A theme has a handful of files; this bounds the walk if somebody points
/// a theme folder at something enormous.
const MAX_ENTRIES: usize = 4096;

pub(crate) struct Watch {
    appearance: Option<PathBuf>,
    theme: Option<PathBuf>,
    stamp: u64,
}

impl Watch {
    /// Watches `appearance.json` and `theme` (the active theme's folder or
    /// file; `None` for the embedded Default).
    pub(crate) fn new(theme: Option<PathBuf>) -> Self {
        let appearance = default_config_dir().map(|dir| dir.join("appearance.json"));
        Self::over(appearance, theme)
    }

    fn over(appearance: Option<PathBuf>, theme: Option<PathBuf>) -> Self {
        let stamp = stamp(appearance.as_deref(), theme.as_deref());
        Watch {
            appearance,
            theme,
            stamp,
        }
    }

    /// Whether anything watched changed since the last call (or `new`).
    pub(crate) fn changed(&mut self) -> bool {
        let now = stamp(self.appearance.as_deref(), self.theme.as_deref());
        std::mem::replace(&mut self.stamp, now) != now
    }

    /// Follows a switch to another theme.
    pub(crate) fn retarget(&mut self, theme: Option<PathBuf>) {
        *self = Self::over(self.appearance.take(), theme);
    }
}

fn stamp(appearance: Option<&Path>, theme: Option<&Path>) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut budget = MAX_ENTRIES;
    for root in [appearance, theme].into_iter().flatten() {
        walk(root, &mut hasher, &mut budget);
    }
    hasher.finish()
}

fn walk(path: &Path, hasher: &mut DefaultHasher, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    path.hash(hasher);
    let Ok(meta) = fs::symlink_metadata(path) else {
        0u8.hash(hasher);
        return;
    };
    meta.len().hash(hasher);
    meta.modified().ok().hash(hasher);
    if meta.is_dir() {
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        children.sort();
        for child in children {
            walk(&child, hasher, budget);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rbx-native-theme-watch-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_edit_a_new_file_and_a_new_selection_are_all_noticed_once() {
        let dir = scratch("edits");
        let appearance = dir.join("appearance.json");
        let theme = dir.join("mine");
        fs::create_dir_all(theme.join("icons")).unwrap();
        fs::write(theme.join("theme.json"), "{}").unwrap();
        let mut watch = Watch::over(Some(appearance.clone()), Some(theme.clone()));
        assert!(!watch.changed());

        fs::write(theme.join("theme.json"), r#"{"colors":{}}"#).unwrap();
        assert!(watch.changed());
        assert!(!watch.changed(), "a change is reported once");

        fs::write(theme.join("icons").join("Part.svg"), "<svg/>").unwrap();
        assert!(watch.changed());

        fs::write(&appearance, r#"{"theme":"mine"}"#).unwrap();
        assert!(watch.changed());
        assert!(!watch.changed());
    }

    #[test]
    fn missing_paths_are_a_stable_state_not_a_change() {
        let dir = scratch("missing");
        let mut watch = Watch::over(Some(dir.join("nope.json")), None);
        assert!(!watch.changed());
        assert!(!watch.changed());
    }
}
