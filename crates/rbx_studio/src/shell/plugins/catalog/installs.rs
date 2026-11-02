//! What the catalog installed: `<config dir>/plugin-catalog.json` maps a
//! store id to the file it wrote in the plugins folder and the store's
//! update time it was fetched at, which is how an update is noticed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Record {
    /// File name inside the plugins folder.
    pub(super) file: String,
    /// The store's `updatedUtc` when this copy was downloaded.
    pub(super) updated: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Installs {
    pub(super) installed: BTreeMap<u64, Record>,
}

impl Installs {
    pub(super) fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub(super) fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        crate::settings::write_atomic(path, text.as_bytes()).map_err(|err| err.to_string())
    }

    /// The record for `id` whose file is still in `dir`; a file the user
    /// deleted by hand makes the plugin not installed again.
    pub(super) fn live(&self, dir: &Path, id: u64) -> Option<&Record> {
        self.installed
            .get(&id)
            .filter(|record| dir.join(&record.file).is_file())
    }
}

/// `name` as a file name: characters no file system takes, and leading
/// dots, dropped; whitespace runs folded. Discovery shows the stem as the
/// plugin's name, so it stays readable.
fn stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let folded = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = folded.trim_start_matches('.').trim();
    if trimmed.is_empty() {
        "Plugin".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Where `id` goes in `dir`: its own earlier file, `<name>.rbxm`, or
/// `<name> (<id>).rbxm` when another plugin already has that name.
pub(super) fn target(dir: &Path, installs: &Installs, id: u64, name: &str) -> PathBuf {
    if let Some(record) = installs.installed.get(&id) {
        return dir.join(&record.file);
    }
    let stem = stem(name);
    let plain = dir.join(format!("{stem}.rbxm"));
    if plain.exists() {
        dir.join(format!("{stem} ({id}).rbxm"))
    } else {
        plain
    }
}

/// Writes the downloaded model to `path` (atomically, so a running host
/// never reads half a file).
pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::settings::write_atomic(path, bytes).map_err(|err| err.to_string())
}

/// True when the store lists another version than the one installed: its
/// update time only ever moves forward, so any change is a newer one. A
/// missing time on either side is not an update.
pub(super) fn has_update(record: &Record, store_updated: &str) -> bool {
    !record.updated.is_empty() && !store_updated.is_empty() && store_updated != record.updated
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rbx-plugin-catalog-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stems_are_safe_and_readable() {
        assert_eq!(stem("Archimedes (v3.1.9)"), "Archimedes (v3.1.9)");
        assert_eq!(stem("Day/Night  Cycle"), "Day Night Cycle");
        assert_eq!(stem("..hidden"), "hidden");
        assert_eq!(stem("<>:"), "Plugin");
    }

    #[test]
    fn target_avoids_someone_elses_file() {
        let dir = TempDir::new("target");
        let installs = Installs::default();
        assert_eq!(
            target(&dir.0, &installs, 7, "Brick Cutter"),
            dir.0.join("Brick Cutter.rbxm")
        );
        std::fs::write(dir.0.join("Brick Cutter.rbxm"), b"x").unwrap();
        assert_eq!(
            target(&dir.0, &installs, 7, "Brick Cutter"),
            dir.0.join("Brick Cutter (7).rbxm")
        );
        let mut mine = Installs::default();
        mine.installed.insert(
            7,
            Record {
                file: "Brick Cutter.rbxm".into(),
                updated: String::new(),
            },
        );
        assert_eq!(
            target(&dir.0, &mine, 7, "Renamed"),
            dir.0.join("Brick Cutter.rbxm")
        );
    }

    #[test]
    fn records_round_trip_and_track_the_file() {
        let dir = TempDir::new("records");
        let path = dir.0.join("plugin-catalog.json");
        let mut installs = Installs::default();
        installs.installed.insert(
            144938633,
            Record {
                file: "Archimedes.rbxm".into(),
                updated: "2024-10-03T15:47:04.703Z".into(),
            },
        );
        installs.save_to(&path).unwrap();
        let loaded = Installs::load_from(&path);
        assert_eq!(loaded, installs);
        assert!(loaded.live(&dir.0, 144938633).is_none());
        write(&dir.0.join("Archimedes.rbxm"), b"rbxm").unwrap();
        assert!(loaded.live(&dir.0, 144938633).is_some());
        assert_eq!(
            Installs::load_from(&dir.0.join("missing.json")),
            Installs::default()
        );
    }

    #[test]
    fn changed_store_time_is_an_update() {
        let record = Record {
            file: "a.rbxm".into(),
            updated: "2024-10-03T15:47:04.703Z".into(),
        };
        assert!(has_update(&record, "2025-01-01T00:00:00Z"));
        assert!(!has_update(&record, "2024-10-03T15:47:04.703Z"));
        assert!(!has_update(&record, ""));
    }
}
