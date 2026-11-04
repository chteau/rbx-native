//! Local version labels. Roblox keeps no name for a place version (the
//! history's `title` is only ever set on a handful of old ones), so a label
//! lives on this machine only, in `version-labels.json` beside `links.json`:
//! place id → version → label. Every editor on the machine shares it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::settings::default_config_dir;

/// Version → label, for one place.
pub(super) type Labels = BTreeMap<u64, String>;

type Store = BTreeMap<u64, Labels>;

fn store_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("version-labels.json"))
}

fn read_store(path: &Path) -> Store {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The labels given to `place_id`'s versions.
pub(super) fn load(place_id: u64) -> Labels {
    store_path()
        .map(|path| load_from(&path, place_id))
        .unwrap_or_default()
}

pub(super) fn load_from(path: &Path, place_id: u64) -> Labels {
    read_store(path).remove(&place_id).unwrap_or_default()
}

/// Labels `version` of `place_id`, or drops its label when `label` is blank.
/// Re-reads the file first, so another editor's labels for the same place
/// survive.
pub(super) fn save(place_id: u64, version: u64, label: &str) -> Result<Labels, String> {
    let path = store_path().ok_or("no config directory to keep labels in")?;
    save_to(&path, place_id, version, label)
}

pub(super) fn save_to(
    path: &Path,
    place_id: u64,
    version: u64,
    label: &str,
) -> Result<Labels, String> {
    let mut store = read_store(path);
    let labels = store.entry(place_id).or_default();
    match label.trim() {
        "" => labels.remove(&version),
        label => labels.insert(version, label.to_string()),
    };
    let labels = labels.clone();
    store.retain(|_, labels| !labels.is_empty());
    let json = serde_json::to_vec_pretty(&store).map_err(|err| err.to_string())?;
    crate::settings::write_atomic(path, &json).map_err(|err| err.to_string())?;
    Ok(labels)
}
