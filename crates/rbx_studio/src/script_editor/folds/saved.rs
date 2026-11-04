//! Which folds of a script were collapsed, remembered on this machine only
//! (Roblox Studio forgets them). Files live under
//! `<config dir>/script-folds/<hash>.json`, keyed like the graph editor's
//! layout: the place file (or `cloud:<placeId>`) plus the script's path.
//!
//! A fold is stored as its start line and that line's trimmed text, and is
//! only collapsed again when the same line is still a fold opener with the
//! same text, so an edit made elsewhere never folds the wrong block.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::script_editor::graph::saved::{file_name, LayoutKey};
use crate::settings::{default_config_dir, write_atomic};

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedFold {
    /// 0-based line of the fold's opener.
    pub(crate) line: usize,
    pub(crate) text: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    folds: Vec<SavedFold>,
}

/// The folds in `lines` (opener lines of `text`), ready to store.
pub(crate) fn capture(lines: &[usize], text: &str) -> Vec<SavedFold> {
    let all: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .filter_map(|&line| {
            Some(SavedFold {
                line,
                text: all.get(line)?.trim().to_owned(),
            })
        })
        .collect()
}

/// The saved folds that still sit on a fold opener (`foldable`, ascending)
/// whose text is unchanged: the lines to collapse.
pub(crate) fn restorable(saved: &[SavedFold], text: &str, foldable: &[usize]) -> Vec<usize> {
    let all: Vec<&str> = text.lines().collect();
    saved
        .iter()
        .filter(|fold| {
            foldable.binary_search(&fold.line).is_ok()
                && all.get(fold.line).is_some_and(|l| l.trim() == fold.text)
        })
        .map(|fold| fold.line)
        .collect()
}

fn root() -> Option<PathBuf> {
    default_config_dir().map(|d| d.join("script-folds"))
}

pub(crate) fn load(key: &LayoutKey) -> Vec<SavedFold> {
    root().map(|dir| load_from(&dir, key)).unwrap_or_default()
}

/// An empty set removes the file.
pub(crate) fn save(key: &LayoutKey, folds: &[SavedFold]) {
    if let Some(dir) = root() {
        save_in(&dir, key, folds);
    }
}

fn load_from(dir: &Path, key: &LayoutKey) -> Vec<SavedFold> {
    std::fs::read(dir.join(file_name(key)))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
        .filter(|file| file.version == VERSION)
        .map(|file| file.folds)
        .unwrap_or_default()
}

fn save_in(dir: &Path, key: &LayoutKey, folds: &[SavedFold]) {
    let path = dir.join(file_name(key));
    // A fold that fails to save is only forgotten, never an error.
    if folds.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let file = File {
        version: VERSION,
        folds: folds.to_vec(),
    };
    if let Ok(bytes) = serde_json::to_vec(&file) {
        let _ = write_atomic(&path, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "local t = {\n\t1,\n\t2,\n}\n\nif x then\n\ty()\nend\n";

    fn key(script: &str) -> LayoutKey {
        LayoutKey {
            place: "/tmp/place.rbxl".to_owned(),
            script: script.to_owned(),
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rbx-folds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn captures_trimmed_opener_text() {
        let saved = capture(&[0, 5], TEXT);
        assert_eq!(saved[0].text, "local t = {");
        assert_eq!(
            saved[1],
            SavedFold {
                line: 5,
                text: "if x then".to_owned()
            }
        );
        assert!(capture(&[99], TEXT).is_empty());
    }

    #[test]
    fn restores_only_unchanged_openers() {
        let saved = capture(&[0, 5], TEXT);
        assert_eq!(restorable(&saved, TEXT, &[0, 5]), vec![0, 5]);
        // The text above shifted everything down a line: nothing matches.
        let shifted = format!("-- new\n{TEXT}");
        assert!(restorable(&saved, &shifted, &[1, 6]).is_empty());
        // Same line, but its text changed.
        let edited = TEXT.replace("if x then", "if y then");
        assert_eq!(restorable(&saved, &edited, &[0, 5]), vec![0]);
        // Same text, but no longer a fold opener.
        assert_eq!(restorable(&saved, TEXT, &[5]), vec![5]);
    }

    #[test]
    fn round_trips_through_a_file_and_empty_deletes_it() {
        let d = dir("round");
        let k = key("Workspace.Main");
        let saved = capture(&[0], TEXT);
        save_in(&d, &k, &saved);
        assert_eq!(load_from(&d, &k), saved);
        assert!(load_from(&d, &key("Workspace.Other")).is_empty());
        save_in(&d, &k, &[]);
        assert!(!d.join(file_name(&k)).exists());
        assert!(load_from(&d, &k).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_of_another_version_is_ignored() {
        let d = dir("version");
        let k = key("Main");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(file_name(&k)),
            r#"{"version":99,"folds":[{"line":0,"text":"x"}]}"#,
        )
        .unwrap();
        assert!(load_from(&d, &k).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
